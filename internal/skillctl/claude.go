package skillctl

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
)

func InstallClaude(root string, manifest Manifest, target string, dryRun bool, out io.Writer) error {
	lock, err := syncLock(root)
	if err != nil {
		return err
	}

	defer releaseLock(lock)
	hidden := map[string]bool{}
	for _, dependency := range manifest.Dependencies {
		if !dependency.Claude {
			for _, name := range topLevelNames(dependency) {
				hidden[name] = true
			}
		}
	}

	sources := map[string]string{}
	for _, name := range manifest.DurableRoots {
		sources[name] = filepath.Join(root, name)
	}

	managed := filepath.Join(root, "_managed")
	managedNames, err := directoryNames(managed)
	if err != nil {
		return err
	}

	for _, name := range managedNames {
		path := filepath.Join(managed, name)
		if !hidden[name] && isDirectory(path) && isFile(filepath.Join(path, "SKILL.md")) {
			sources[name] = path
		}
	}

	if err := os.MkdirAll(target, 0o755); err != nil {
		return err
	}

	ownedPath := filepath.Join(target, ".skillctl-owned")
	ownedBytes, err := os.ReadFile(ownedPath)
	if err != nil && !os.IsNotExist(err) {
		return err
	}

	owned := map[string]bool{}
	for _, name := range strings.Fields(string(ownedBytes)) {
		if _, err := safeName(name, "Claude ownership entry"); err != nil {
			return err
		}

		owned[name] = true
	}

	names := make([]string, 0, len(sources))
	for name := range sources {
		names = append(names, name)
		if _, err := os.Lstat(filepath.Join(target, name)); err == nil && !owned[name] {
			return fmt.Errorf("%s exists and is not managed by skillctl", filepath.Join(target, name))
		} else if err != nil && !os.IsNotExist(err) {
			return err
		}
	}

	slices.Sort(names)
	stage, err := os.MkdirTemp(target, ".skillctl-stage-")
	if err != nil {
		return err
	}

	defer os.RemoveAll(stage)
	changed := []string{}
	for _, name := range names {
		source := sources[name]
		destination := filepath.Join(target, name)
		equal, err := treesEqual(source, destination)
		if err != nil {
			return err
		}

		content, err := os.ReadFile(filepath.Join(source, "SKILL.md"))
		if err != nil {
			return err
		}

		// Normalized managed skills need neither copying nor name rewriting.
		if equal && owned[name] && hasSkillName(content, name) {
			continue
		}

		staged := filepath.Join(stage, name)
		if err := copySkillTree(source, staged, ""); err != nil {
			return err
		}

		if err := setSkillName(staged, name); err != nil {
			return err
		}

		equal, err = treesEqual(staged, destination)
		if err != nil {
			return err
		}

		if !equal {
			changed = append(changed, name)
			if owned[name] {
				fmt.Fprintln(out, "UPDATE", name)
			}
		}
	}

	removed := []string{}
	for name := range owned {
		if _, exists := sources[name]; !exists {
			removed = append(removed, name)
		}
	}

	slices.Sort(removed)
	for _, name := range names {
		if !owned[name] {
			fmt.Fprintln(out, "ADD", name)
		}
	}

	for _, name := range removed {
		fmt.Fprintln(out, "REMOVE", name)
	}

	if dryRun {
		fmt.Fprintf(out, "Would install %d skills into %s\n", len(names), target)
		return nil
	}

	// Record new ownership before moves so an interrupted run can safely retry.
	union := append([]string{}, names...)
	for _, name := range removed {
		union = append(union, name)
	}

	slices.Sort(union)
	if err := os.WriteFile(ownedPath, []byte(strings.Join(union, "\n")+"\n"), 0o644); err != nil {
		return err
	}

	for _, name := range removed {
		if err := os.RemoveAll(filepath.Join(target, name)); err != nil {
			return err
		}
	}

	for _, name := range changed {
		destination := filepath.Join(target, name)
		if err := os.RemoveAll(destination); err != nil {
			return err
		}

		if err := os.Rename(filepath.Join(stage, name), destination); err != nil {
			return err
		}
	}

	if err := os.WriteFile(ownedPath, []byte(strings.Join(names, "\n")+"\n"), 0o644); err != nil {
		return err
	}

	fmt.Fprintf(out, "Installed %d skills into %s\n", len(names), target)
	return nil
}

func hasSkillName(content []byte, name string) bool {
	match := frontmatter.FindSubmatch(content)
	if match == nil {
		return false
	}

	for _, line := range strings.Split(string(match[1]), "\n") {
		if strings.HasPrefix(line, "name:") {
			return line == "name: "+name
		}
	}

	return false
}
