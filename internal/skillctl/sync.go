package skillctl

import (
	"context"
	"crypto/sha256"
	"embed"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strings"
	"sync"
)

// Embed production sources so renderer changes invalidate state in installed
// binaries, just as Python source changes did. Templates ship inside the binary.
//
//go:embed sync.go tree.go generation.go manifest.go git.go lock.go claude.go templates/*.tmpl
var generatorSources embed.FS

type selectedSkill struct {
	source     string
	relative   string
	repository string
	routed     bool
}

type selectedRouter struct {
	router  Router
	members []string
}

func Sync(ctx context.Context, root string, manifest Manifest, dryRun bool, out io.Writer) error {
	lock, err := syncLock(root)
	if err != nil {
		return err
	}

	defer releaseLock(lock)
	cacheRoot, err := gitCacheRoot()
	if err != nil {
		return err
	}

	if err := os.MkdirAll(cacheRoot, 0o755); err != nil {
		return err
	}

	cacheLock, err := syncLock(cacheRoot)
	if err != nil {
		return err
	}

	defer releaseLock(cacheLock)
	lockContent, err := io.ReadAll(lock)
	if err != nil {
		return err
	}

	var previous syncRecord
	if err := json.Unmarshal(lockContent, &previous); err != nil || previous.Version != 1 {
		previous = syncRecord{}
	}

	// Workers own distinct cache entries; the shared lock stays held through copy.
	type acquisition struct {
		root string
		err  error
	}

	acquired := make([]acquisition, len(manifest.Dependencies))
	jobs := make(chan int)
	var workers sync.WaitGroup
	for range min(4, len(manifest.Dependencies)) {
		workers.Add(1)
		go func() {
			defer workers.Done()
			for index := range jobs {
				acquired[index].root, acquired[index].err = acquireDependency(ctx, manifest.Dependencies[index], cacheRoot)
			}
		}()
	}

	for index := range manifest.Dependencies {
		jobs <- index
	}

	close(jobs)
	workers.Wait()
	if err := ctx.Err(); err != nil {
		return err
	}

	for _, result := range acquired {
		if result.err != nil {
			return result.err
		}
	}

	temporary, err := os.MkdirTemp(filepath.Dir(root), ".skills-sync-")
	if err != nil {
		return err
	}

	keepBackup := false
	defer func() {
		if !keepBackup {
			os.RemoveAll(temporary)
		}
	}()
	stage := filepath.Join(temporary, "_managed")
	if err := os.Mkdir(stage, 0o755); err != nil {
		return err
	}

	generatorHash := sha256.New()
	if err := fs.WalkDir(generatorSources, ".", func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil || entry.IsDir() {
			return walkErr
		}

		content, err := generatorSources.ReadFile(path)
		if err != nil {
			return err
		}

		fmt.Fprintf(generatorHash, "%s\x00%d\x00", path, len(content))
		generatorHash.Write(content)
		return nil
	}); err != nil {
		return err
	}

	inputs := generationInputs{
		Generator:    "skillctl-go:" + hex.EncodeToString(generatorHash.Sum(nil)),
		DurableRoots: append([]string{}, manifest.DurableRoots...),
		Dependencies: []dependencyInput{},
	}
	sort.Strings(inputs.DurableRoots)
	resolved := []resolvedDependency{}
	selected := []selectedSkill{}
	routers := []selectedRouter{}
	destinations := map[string]bool{}
	durable := map[string]bool{}
	for _, name := range manifest.DurableRoots {
		durable[name] = true
	}

	for index, dependency := range manifest.Dependencies {
		clone := acquired[index].root
		mappings := dependency.Selection.Include
		if dependency.Selection.Mode == "discovery" {
			mappings = nil
			discoveryRoot := filepath.Join(clone, filepath.FromSlash(dependency.Selection.Root))
			entries, err := os.ReadDir(discoveryRoot)
			if err != nil {
				return fmt.Errorf("%s discovery root does not exist: %s: %w", dependency.ID, dependency.Selection.Root, err)
			}

			excluded := map[string]bool{}
			for _, name := range dependency.Selection.Exclude {
				excluded[name] = true
			}

			for _, entry := range entries {
				path := filepath.Join(discoveryRoot, entry.Name())
				if !excluded[entry.Name()] && isDirectory(path) && isFile(filepath.Join(path, "SKILL.md")) {
					source, err := filepath.Rel(clone, path)
					if err != nil {
						return err
					}

					mappings = append(mappings, Mapping{Source: filepath.ToSlash(source), Destination: entry.Name()})
				}
			}
		}

		if dependency.Router != nil {
			name := dependency.Router.Name
			if durable[name] || destinations[name] {
				return fmt.Errorf("duplicate destination or authored skill collision: %s", name)
			}

			destinations[name] = true
			members := []string{}
			for _, mapping := range mappings {
				members = append(members, mapping.Destination)
			}

			routers = append(routers, selectedRouter{router: *dependency.Router, members: members})
		}

		paths := []string{}
		hasSymlinks := false
		for _, mapping := range mappings {
			relative := mapping.Destination
			if dependency.Router != nil {
				relative = filepath.Join(dependency.Router.Name, "skills", relative)
			} else {
				if durable[relative] || destinations[relative] {
					return fmt.Errorf("duplicate destination or authored skill collision: %s", relative)
				}

				destinations[relative] = true
			}

			source := filepath.Join(clone, filepath.FromSlash(mapping.Source))
			if !isDirectory(source) || !isFile(filepath.Join(source, "SKILL.md")) {
				return fmt.Errorf("%s source is not a skill: %s", dependency.ID, mapping.Source)
			}

			resolvedSource, err := filepath.EvalSymlinks(source)
			if err != nil {
				return err
			}

			resolvedClone, err := filepath.EvalSymlinks(clone)
			if err != nil || !containedPath(resolvedClone, resolvedSource) {
				return fmt.Errorf("source symlink escapes repository: %s", source)
			}

			for ancestor := source; containedPath(clone, ancestor); ancestor = filepath.Dir(ancestor) {
				info, err := os.Lstat(ancestor)
				if err != nil {
					return err
				}

				if info.Mode()&os.ModeSymlink != 0 {
					hasSymlinks = true
				}

				if ancestor == clone {
					break
				}
			}

			if err := filepath.WalkDir(source, func(path string, entry fs.DirEntry, walkErr error) error {
				if walkErr != nil {
					return walkErr
				}

				if entry.Type()&os.ModeSymlink != 0 {
					hasSymlinks = true
					resolved, err := filepath.EvalSymlinks(path)
					if err != nil || !containedPath(resolvedClone, resolved) {
						return fmt.Errorf("source symlink escapes repository: %s", path)
					}
				}

				return nil
			}); err != nil {
				return err
			}

			selected = append(selected, selectedSkill{source: source, relative: relative, repository: clone, routed: dependency.Router != nil})
			paths = append(paths, mapping.Source)
		}

		if dependency.Selection.Mode == "discovery" {
			paths = []string{dependency.Selection.Root}
		}

		if hasSymlinks {
			paths = []string{"."}
		}

		args := []string{"rev-parse", "HEAD"}
		for _, path := range paths {
			if path == "." {
				args = append(args, "HEAD^{tree}")
			} else {
				args = append(args, "HEAD:"+path)
			}
		}

		revisionOutput, err := runGit(ctx, clone, args...)
		if err != nil {
			return err
		}

		revisions := strings.Split(revisionOutput, "\n")
		inputs.Dependencies = append(inputs.Dependencies, dependencyInput{Config: dependency, Trees: revisions[1:]})
		ref := dependency.Ref
		if ref == "" {
			ref = "HEAD"
		}

		resolved = append(resolved, resolvedDependency{ID: dependency.ID, URL: dependency.URL, Ref: ref, Commit: revisions[0]})
	}

	current := filepath.Join(root, "_managed")
	liveFingerprint, err := treeFingerprint(current)
	if err != nil {
		return err
	}

	state := generationState{Inputs: inputs, Output: liveFingerprint}
	record := syncRecord{Version: 1, Dependencies: resolved, State: state}

	if liveFingerprint != "" && recordsEqual(previous.State, state) {
		if !dryRun && !recordsEqual(previous, record) {
			content, err := renderSyncLock(record)
			if err != nil {
				return err
			}

			if err := writeSyncLock(lock, content); err != nil {
				return err
			}
		}

		action := "Installed"
		if dryRun {
			action = "Would install"
		}

		fmt.Fprintf(out, "%s %d managed skills (unchanged)\n", action, len(destinations))
		return nil
	}

	for _, skill := range selected {
		target := filepath.Join(stage, skill.relative)
		if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
			return err
		}

		if err := copySkillTree(skill.source, target, skill.repository); err != nil {
			return err
		}

		if err := setSkillName(target, filepath.Base(skill.relative)); err != nil {
			return err
		}

		if skill.routed {
			if err := routeGuides(target); err != nil {
				return err
			}
		}
	}

	for _, router := range routers {
		if err := writeRouter(filepath.Join(stage, router.router.Name), router.router, router.members); err != nil {
			return err
		}
	}

	oldNames, err := directoryNames(current)
	if err != nil {
		return err
	}

	newNames, err := directoryNames(stage)
	if err != nil {
		return err
	}

	for _, name := range newNames {
		if !slices.Contains(oldNames, name) {
			fmt.Fprintln(out, "ADD", name)
			continue
		}

		equal, err := treesEqual(filepath.Join(current, name), filepath.Join(stage, name))
		if err != nil {
			return err
		}

		if !equal {
			fmt.Fprintln(out, "UPDATE", name)
		}
	}

	for _, name := range oldNames {
		if !slices.Contains(newNames, name) {
			fmt.Fprintln(out, "REMOVE", name)
		}
	}

	if dryRun {
		fmt.Fprintf(out, "Would install %d managed skills\n", len(newNames))
		return nil
	}

	if err := ctx.Err(); err != nil {
		return err
	}

	stagedFingerprint, err := treeFingerprint(stage)
	if err != nil {
		return err
	}

	record.State.Output = stagedFingerprint
	lockContent, err = renderSyncLock(record)
	if err != nil {
		return err
	}

	if liveFingerprint != stagedFingerprint {
		backup := filepath.Join(temporary, "previous-managed")
		movedOld := false
		if _, err := os.Lstat(current); err == nil {
			if err := os.Rename(current, backup); err != nil {
				return err
			}

			movedOld = true
		} else if !os.IsNotExist(err) {
			return err
		}

		if err := os.Rename(stage, current); err != nil {
			if movedOld {
				if restoreErr := os.Rename(backup, current); restoreErr != nil {
					keepBackup = true
					return fmt.Errorf("install failed: %v; restore failed: %v; backup: %s", err, restoreErr, backup)
				}
			}

			return err
		}
	}

	if err := writeSyncLock(lock, lockContent); err != nil {
		return err
	}

	fmt.Fprintf(out, "Installed %d managed skills\n", len(newNames))
	return nil
}
