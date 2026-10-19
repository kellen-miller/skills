package skillctl

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

const fixtureURL = "https://github.com/example/source.git"

type syncFixture struct {
	t                   *testing.T
	root, source, cache string
	manifest            Manifest
}

func newSyncFixture(t *testing.T) *syncFixture {
	t.Helper()
	base := t.TempDir()
	f := &syncFixture{t: t, root: filepath.Join(base, "skills"), source: filepath.Join(base, "source"), cache: filepath.Join(base, "cache")}
	f.write(filepath.Join(f.source, "skills/alpha/SKILL.md"), "---\nname: alpha\n---\n")
	if err := os.MkdirAll(f.root, 0755); err != nil {
		t.Fatal(err)
	}

	f.git("init", "-b", "main")
	f.git("config", "user.name", "Test User")
	f.git("config", "user.email", "test@example.com")
	f.git("config", "commit.gpgsign", "false")
	f.commit()
	t.Setenv("GIT_CONFIG_COUNT", "1")
	t.Setenv("GIT_CONFIG_KEY_0", "url.file://"+f.source+".insteadOf")
	t.Setenv("GIT_CONFIG_VALUE_0", fixtureURL)
	t.Setenv("XDG_CACHE_HOME", f.cache)
	t.Setenv("GIT_TERMINAL_PROMPT", "0")
	f.manifest = Manifest{Dependencies: []Dependency{{ID: "example", URL: fixtureURL, Ref: "HEAD", Claude: true, Selection: Selection{Mode: "explicit", Include: []Mapping{{Source: "skills/alpha", Destination: "alpha"}}}}}}
	return f
}

func (f *syncFixture) write(path, content string) {
	f.t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		f.t.Fatal(err)
	}

	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		f.t.Fatal(err)
	}
}

func (f *syncFixture) git(args ...string) string {
	f.t.Helper()
	cmd := exec.Command("git", append([]string{"-C", f.source}, args...)...)
	output, err := cmd.CombinedOutput()
	if err != nil {
		f.t.Fatalf("git %v: %v\n%s", args, err, output)
	}

	return strings.TrimSpace(string(output))
}

func (f *syncFixture) commit() {
	f.t.Helper()
	f.git("add", "-A")
	f.git("commit", "-qm", "Update fixture")
}

func (f *syncFixture) sync(dryRun bool) (string, error) {
	f.t.Helper()
	var output bytes.Buffer
	err := Sync(context.Background(), f.root, f.manifest, dryRun, &output)
	return output.String(), err
}

func (f *syncFixture) mustSync() string {
	f.t.Helper()
	output, err := f.sync(false)
	if err != nil {
		f.t.Fatalf("sync: %v\n%s", err, output)
	}

	return output
}

func readSyncFile(t *testing.T, path string) []byte {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	return data
}

func statSyncFile(t *testing.T, path string) os.FileInfo {
	t.Helper()
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}

	return info
}

func assertAbsent(t *testing.T, path string) {
	t.Helper()
	if _, err := os.Lstat(path); !os.IsNotExist(err) {
		t.Fatalf("expected %s absent, got %v", path, err)
	}
}

func assertSameSyncFile(t *testing.T, before os.FileInfo, path string) {
	t.Helper()
	after := statSyncFile(t, path)
	if !os.SameFile(before, after) || !before.ModTime().Equal(after.ModTime()) {
		t.Fatalf("unchanged file replaced or touched: %s", path)
	}
}

type fixtureLock struct {
	Version      int `json:"version"`
	Dependencies []struct {
		ID     string `json:"id"`
		URL    string `json:"url"`
		Ref    string `json:"ref"`
		Commit string `json:"commit"`
	} `json:"dependencies"`
	State map[string]json.RawMessage `json:"state"`
}

func readFixtureLock(t *testing.T, root string) fixtureLock {
	t.Helper()
	var lock fixtureLock
	if err := json.Unmarshal(readSyncFile(t, filepath.Join(root, ".skillctl.lock")), &lock); err != nil {
		t.Fatal(err)
	}

	return lock
}

func TestSyncUnchangedPreservesFilesAndRecordsCommit(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	path := filepath.Join(f.root, "_managed/alpha/SKILL.md")
	before := statSyncFile(t, path)
	lockPath := filepath.Join(f.root, ".skillctl.lock")
	lockBefore := statSyncFile(t, lockPath)
	output := f.mustSync()
	assertSameSyncFile(t, before, path)
	if !os.SameFile(lockBefore, statSyncFile(t, lockPath)) {
		t.Fatal("lock inode replaced")
	}

	for _, action := range []string{"ADD ", "UPDATE ", "REMOVE "} {
		if strings.Contains(output, action) {
			t.Fatalf("unchanged sync reports %s: %s", action, output)
		}
	}

	lock := readFixtureLock(t, f.root)
	if lock.Version != 1 || len(lock.Dependencies) != 1 {
		t.Fatalf("invalid lock: %+v", lock)
	}

	dep := lock.Dependencies[0]
	if dep.ID != "example" || dep.URL != fixtureURL || dep.Ref != "HEAD" || dep.Commit != f.git("rev-parse", "HEAD") {
		t.Fatalf("incorrect resolved dependency: %+v", dep)
	}

	if len(lock.State["inputs"]) == 0 || len(lock.State["output"]) == 0 {
		t.Fatalf("missing generation state: %+v", lock.State)
	}

	assertAbsent(t, filepath.Join(f.root, ".skillctl-state.json"))
}

func TestSyncUnselectedChangeUpdatesLockWithoutReplacingOutput(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	path := filepath.Join(f.root, "_managed/alpha/SKILL.md")
	before := statSyncFile(t, path)
	old := readFixtureLock(t, f.root)
	f.write(filepath.Join(f.source, "README.md"), "Unrelated upstream change")
	f.commit()
	f.mustSync()
	assertSameSyncFile(t, before, path)
	next := readFixtureLock(t, f.root)
	if old.Dependencies[0].Commit == next.Dependencies[0].Commit || next.Dependencies[0].Commit != f.git("rev-parse", "HEAD") {
		t.Fatal("lock did not follow upstream commit")
	}

	for _, key := range []string{"inputs", "output"} {
		if !bytes.Equal(old.State[key], next.State[key]) {
			t.Fatalf("unselected change altered generation %s", key)
		}
	}
}

func TestSyncUpdatesAndDeletesSelectedFiles(t *testing.T) {
	f := newSyncFixture(t)
	extra := filepath.Join(f.source, "skills/alpha/obsolete.md")
	f.write(extra, "Obsolete")
	f.commit()
	f.mustSync()
	f.write(filepath.Join(f.source, "skills/alpha/SKILL.md"), "Updated upstream skill\n")
	if err := os.Remove(extra); err != nil {
		t.Fatal(err)
	}

	f.commit()
	f.mustSync()
	if !bytes.Contains(readSyncFile(t, filepath.Join(f.root, "_managed/alpha/SKILL.md")), []byte("Updated upstream skill")) {
		t.Fatal("upstream update missing")
	}

	assertAbsent(t, filepath.Join(f.root, "_managed/alpha/obsolete.md"))
}

func TestSyncDestinationAndRouterChanges(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	f.manifest.Dependencies[0].Selection.Include[0].Destination = "renamed"
	f.mustSync()
	assertAbsent(t, filepath.Join(f.root, "_managed/alpha"))
	if !bytes.Contains(readSyncFile(t, filepath.Join(f.root, "_managed/renamed/SKILL.md")), []byte("name: renamed")) {
		t.Fatal("destination name not rewritten")
	}

	f.manifest.Dependencies[0].Router = &Router{Name: "vendor", Description: "Vendor skills"}
	f.mustSync()
	assertAbsent(t, filepath.Join(f.root, "_managed/renamed"))
	statSyncFile(t, filepath.Join(f.root, "_managed/vendor/skills/renamed/GUIDE.md"))
	statSyncFile(t, filepath.Join(f.root, "_managed/vendor/SKILL.md"))
}

func TestSyncDiscoveryRemovesRenamedSkillsAndDependencies(t *testing.T) {
	f := newSyncFixture(t)
	f.manifest.Dependencies[0].Selection = Selection{Mode: "discovery", Root: "skills"}
	f.mustSync()
	if err := os.Rename(filepath.Join(f.source, "skills/alpha"), filepath.Join(f.source, "skills/beta")); err != nil {
		t.Fatal(err)
	}

	f.commit()
	f.mustSync()
	assertAbsent(t, filepath.Join(f.root, "_managed/alpha"))
	statSyncFile(t, filepath.Join(f.root, "_managed/beta/SKILL.md"))
	f.manifest.Dependencies = nil
	f.mustSync()
	entries, err := os.ReadDir(filepath.Join(f.root, "_managed"))
	if err != nil || len(entries) != 0 {
		t.Fatalf("managed tree not cleared: %v, %v", entries, err)
	}

	if len(readFixtureLock(t, f.root).Dependencies) != 0 {
		t.Fatal("removed dependency retained in lock")
	}
}

func TestSyncRepairsLocalDriftAndModes(t *testing.T) {
	f := newSyncFixture(t)
	script := filepath.Join(f.source, "skills/alpha/run.sh")
	f.write(script, "#!/bin/sh\nexit 0\n")
	if err := os.Chmod(script, 0755); err != nil {
		t.Fatal(err)
	}

	f.commit()
	f.mustSync()
	managed := filepath.Join(f.root, "_managed")
	installed := filepath.Join(managed, "alpha/SKILL.md")
	f.write(installed, "Local edit")
	f.write(filepath.Join(managed, "obsolete/extra.md"), "Unexpected directory")
	f.write(filepath.Join(managed, "alpha/.git/config"), "Unexpected git metadata")
	if err := os.Chmod(filepath.Join(managed, "alpha/run.sh"), 0644); err != nil {
		t.Fatal(err)
	}

	f.mustSync()
	if !bytes.Contains(readSyncFile(t, installed), []byte("name: alpha")) {
		t.Fatal("local edit not repaired")
	}

	assertAbsent(t, filepath.Join(managed, "obsolete"))
	assertAbsent(t, filepath.Join(managed, "alpha/.git"))
	if statSyncFile(t, filepath.Join(managed, "alpha/run.sh")).Mode().Perm()&0111 == 0 {
		t.Fatal("executable mode not restored")
	}

	if err := os.Remove(installed); err != nil {
		t.Fatal(err)
	}

	f.mustSync()
	statSyncFile(t, installed)
}

func TestSyncRecoversCacheAndCorruptState(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	path := filepath.Join(f.root, "_managed/alpha/SKILL.md")
	before := statSyncFile(t, path)
	if err := os.RemoveAll(filepath.Join(f.cache, "skillctl")); err != nil {
		t.Fatal(err)
	}

	f.mustSync()
	gitDirs, err := filepath.Glob(filepath.Join(f.cache, "skillctl/repos/*/.git"))
	if err != nil || len(gitDirs) == 0 {
		t.Fatalf("cache not regenerated: %v %v", gitDirs, err)
	}

	for _, gitDir := range gitDirs {
		if err := os.RemoveAll(gitDir); err != nil {
			t.Fatal(err)
		}
	}

	f.mustSync()
	f.write(filepath.Join(f.root, ".skillctl.lock"), "broken")
	f.mustSync()
	assertSameSyncFile(t, before, path)
	readFixtureLock(t, f.root)
}

func TestSyncDryRunAndFailedRefreshPreserveLiveState(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	installed := filepath.Join(f.root, "_managed/alpha/SKILL.md")
	before := statSyncFile(t, installed)
	lockPath := filepath.Join(f.root, ".skillctl.lock")
	lock := readSyncFile(t, lockPath)
	f.write(filepath.Join(f.source, "skills/alpha/SKILL.md"), "Changed upstream\n")
	f.commit()
	output, err := f.sync(true)
	if err != nil || !strings.Contains(output, "UPDATE alpha") {
		t.Fatalf("dry run: %v\n%s", err, output)
	}

	assertSameSyncFile(t, before, installed)
	if !bytes.Equal(lock, readSyncFile(t, lockPath)) {
		t.Fatal("dry run advanced lock")
	}

	if err := os.Rename(f.source, f.source+"-offline"); err != nil {
		t.Fatal(err)
	}

	if _, err := f.sync(false); err == nil {
		t.Fatal("sync accepted stale cache after fetch failure")
	}

	assertSameSyncFile(t, before, installed)
	if !bytes.Equal(lock, readSyncFile(t, lockPath)) {
		t.Fatal("failed refresh advanced lock")
	}
}

func TestSyncMissingSelectedSkillPreservesOutput(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	installed := filepath.Join(f.root, "_managed/alpha/SKILL.md")
	before := statSyncFile(t, installed)
	lock := readSyncFile(t, filepath.Join(f.root, ".skillctl.lock"))
	if err := os.RemoveAll(filepath.Join(f.source, "skills/alpha")); err != nil {
		t.Fatal(err)
	}

	f.write(filepath.Join(f.source, "README.md"), "Skill removed")
	f.commit()
	if _, err := f.sync(false); err == nil {
		t.Fatal("missing explicit source accepted")
	}

	assertSameSyncFile(t, before, installed)
	if !bytes.Equal(lock, readSyncFile(t, filepath.Join(f.root, ".skillctl.lock"))) {
		t.Fatal("failed generation advanced lock")
	}
}

func TestSyncPinnedCommitAndForceUpdatedRef(t *testing.T) {
	f := newSyncFixture(t)
	pinned := f.git("rev-parse", "HEAD")
	f.manifest.Dependencies[0].Ref = pinned
	f.mustSync()
	f.write(filepath.Join(f.source, "skills/alpha/SKILL.md"), "New branch contents\n")
	f.commit()
	f.mustSync()
	installed := filepath.Join(f.root, "_managed/alpha/SKILL.md")
	if bytes.Contains(readSyncFile(t, installed), []byte("New branch contents")) {
		t.Fatal("pinned commit followed branch")
	}

	f.manifest.Dependencies[0].Ref = "main"
	f.mustSync()
	f.write(filepath.Join(f.source, "skills/alpha/SKILL.md"), "Rewritten history\n")
	f.git("add", "-A")
	f.git("commit", "--amend", "--no-edit", "-q")
	f.mustSync()
	if !bytes.Contains(readSyncFile(t, installed), []byte("Rewritten history")) {
		t.Fatal("force updated ref not followed")
	}
}

func TestSyncFollowsRemoteDefaultBranch(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	f.git("checkout", "-qb", "replacement")
	f.write(filepath.Join(f.source, "skills/alpha/SKILL.md"), "New default branch\n")
	f.commit()
	f.mustSync()
	if !bytes.Contains(readSyncFile(t, filepath.Join(f.root, "_managed/alpha/SKILL.md")), []byte("New default branch")) {
		t.Fatal("HEAD did not follow remote default branch")
	}
}

func TestSyncSparseSelectionChangeUsesNewContents(t *testing.T) {
	f := newSyncFixture(t)
	f.write(filepath.Join(f.source, "other/beta/SKILL.md"), "---\nname: beta\n---\nOther selection\n")
	f.commit()
	f.manifest.Dependencies[0].Sparse = []string{"skills"}
	f.mustSync()
	f.manifest.Dependencies[0].Sparse = []string{"other"}
	f.manifest.Dependencies[0].Selection.Include = []Mapping{{Source: "other/beta", Destination: "beta"}}
	f.mustSync()
	assertAbsent(t, filepath.Join(f.root, "_managed/alpha"))
	if !bytes.Contains(readSyncFile(t, filepath.Join(f.root, "_managed/beta/SKILL.md")), []byte("Other selection")) {
		t.Fatal("changed sparse selection missing")
	}
}

func TestSyncUpdatesSymlinkTargetsAndAliasedSources(t *testing.T) {
	f := newSyncFixture(t)
	f.write(filepath.Join(f.source, "shared.md"), "Original reference")
	if err := os.Symlink("../../shared.md", filepath.Join(f.source, "skills/alpha/reference.md")); err != nil {
		t.Fatal(err)
	}

	if err := os.Symlink("skills", filepath.Join(f.source, "alias")); err != nil {
		t.Fatal(err)
	}

	f.commit()
	f.manifest.Dependencies[0].Selection.Include[0].Source = "alias/alpha"
	f.mustSync()
	f.write(filepath.Join(f.source, "shared.md"), "Updated external reference")
	f.commit()
	f.mustSync()
	if string(readSyncFile(t, filepath.Join(f.root, "_managed/alpha/reference.md"))) != "Updated external reference" {
		t.Fatal("symlink target update not detected")
	}
}

func TestSyncRouterGuideCollisionPreservesOutput(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	installed := filepath.Join(f.root, "_managed/alpha/SKILL.md")
	before := statSyncFile(t, installed)
	f.write(filepath.Join(f.source, "skills/alpha/GUIDE.md"), "Existing reference")
	f.commit()
	f.manifest.Dependencies[0].Router = &Router{Name: "vendor", Description: "Vendor skills"}
	if _, err := f.sync(false); err == nil {
		t.Fatal("router overwrote existing GUIDE.md")
	}

	assertSameSyncFile(t, before, installed)
	assertAbsent(t, filepath.Join(f.root, "_managed/vendor"))
}

func TestSyncLockExcludesAnotherProcess(t *testing.T) {
	if root := os.Getenv("SKILLCTL_TEST_LOCK_ROOT"); root != "" {
		lock, err := syncLock(root)
		if err == nil {
			releaseLock(lock)
			os.Exit(1)
		}

		os.Exit(0)
	}

	f := newSyncFixture(t)
	f.mustSync()
	lock, err := syncLock(f.root)
	if err != nil {
		t.Fatal(err)
	}

	defer releaseLock(lock)
	child := exec.Command(os.Args[0], "-test.run=^TestSyncLockExcludesAnotherProcess$")
	child.Env = append(os.Environ(), "SKILLCTL_TEST_LOCK_ROOT="+f.root)
	if output, err := child.CombinedOutput(); err != nil {
		t.Fatalf("second process acquired held lock: %v\n%s", err, output)
	}

	if _, err := f.sync(false); err == nil {
		t.Fatal("sync acquired held lock")
	}
}

func TestSyncSharedCacheLockProtectsCachedCheckout(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	installed := filepath.Join(f.root, "_managed/alpha/SKILL.md")
	before := statSyncFile(t, installed)
	lock, err := syncLock(filepath.Join(f.cache, "skillctl"))
	if err != nil {
		t.Fatal(err)
	}

	defer releaseLock(lock)
	if _, err := f.sync(false); err == nil {
		t.Fatal("sync bypassed shared cache lock")
	}

	assertSameSyncFile(t, before, installed)
}

func TestClaudeInstallPreservesUnchangedFilesAndLock(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	f.write(filepath.Join(f.root, "authored/SKILL.md"), "---\nname: authored\n---\n")
	f.manifest.DurableRoots = []string{"authored"}
	target := filepath.Join(t.TempDir(), "claude")
	lock := readSyncFile(t, filepath.Join(f.root, ".skillctl.lock"))
	if err := InstallClaude(f.root, f.manifest, target, false, &bytes.Buffer{}); err != nil {
		t.Fatal(err)
	}

	path := filepath.Join(target, "authored/SKILL.md")
	before := statSyncFile(t, path)
	if err := InstallClaude(f.root, f.manifest, target, false, &bytes.Buffer{}); err != nil {
		t.Fatal(err)
	}

	assertSameSyncFile(t, before, path)
	if !bytes.Equal(lock, readSyncFile(t, filepath.Join(f.root, ".skillctl.lock"))) {
		t.Fatal("Claude install rewrote dependency lock")
	}

	f.write(filepath.Join(target, "unowned/keep.md"), "Keep")
	f.write(filepath.Join(f.root, "authored/extra.md"), "New reference")
	if err := os.RemoveAll(filepath.Join(f.root, "_managed/alpha")); err != nil {
		t.Fatal(err)
	}

	if err := InstallClaude(f.root, f.manifest, target, false, &bytes.Buffer{}); err != nil {
		t.Fatal(err)
	}

	statSyncFile(t, filepath.Join(target, "authored/extra.md"))
	statSyncFile(t, filepath.Join(target, "unowned/keep.md"))
	assertAbsent(t, filepath.Join(target, "alpha"))
}

func TestClaudeInstallProtectsUnownedEntries(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	target := filepath.Join(t.TempDir(), "claude")
	path := filepath.Join(target, "alpha/SKILL.md")
	f.write(path, "Mine")
	if err := InstallClaude(f.root, f.manifest, target, false, &bytes.Buffer{}); err == nil {
		t.Fatal("unowned skill overwritten")
	}

	if string(readSyncFile(t, path)) != "Mine" {
		t.Fatal("unowned content changed")
	}
}

func TestClaudeInstallDryRunAndHiddenDependency(t *testing.T) {
	f := newSyncFixture(t)
	f.mustSync()
	target := filepath.Join(t.TempDir(), "claude")
	if err := InstallClaude(f.root, f.manifest, target, true, &bytes.Buffer{}); err != nil {
		t.Fatal(err)
	}

	assertAbsent(t, filepath.Join(target, "alpha"))
	if err := InstallClaude(f.root, f.manifest, target, false, &bytes.Buffer{}); err != nil {
		t.Fatal(err)
	}

	f.manifest.Dependencies[0].Claude = false
	if err := InstallClaude(f.root, f.manifest, target, false, &bytes.Buffer{}); err != nil {
		t.Fatal(err)
	}

	assertAbsent(t, filepath.Join(target, "alpha"))
}

func TestSyncRejectsTrailingLockData(t *testing.T) {
	for _, suffix := range []string{"\ntrailing garbage", "\n{}"} {
		t.Run(suffix, func(t *testing.T) {
			f := newSyncFixture(t)
			f.mustSync()
			lockPath := filepath.Join(f.root, ".skillctl.lock")
			before := statSyncFile(t, lockPath)
			f.write(lockPath, string(readSyncFile(t, lockPath))+suffix)
			f.mustSync()
			var record map[string]any
			if err := json.Unmarshal(readSyncFile(t, lockPath), &record); err != nil {
				t.Fatalf("corrupt lock was not repaired: %v", err)
			}

			if !os.SameFile(before, statSyncFile(t, lockPath)) {
				t.Fatal("repair replaced the locked inode")
			}
		})
	}
}

func TestSyncRejectsEscapingAndCyclicSymlinks(t *testing.T) {
	for _, cycle := range []bool{false, true} {
		name := "escape"
		if cycle {
			name = "cycle"
		}

		t.Run(name, func(t *testing.T) {
			f := newSyncFixture(t)
			f.mustSync()
			installed := filepath.Join(f.root, "_managed/alpha/SKILL.md")
			before := statSyncFile(t, installed)
			lockBefore := readSyncFile(t, filepath.Join(f.root, ".skillctl.lock"))
			target := filepath.Join(t.TempDir(), "outside.md")
			f.write(target, "Outside repository")
			if cycle {
				target = "."
			}

			if err := os.Symlink(target, filepath.Join(f.source, "skills/alpha/unsafe")); err != nil {
				t.Fatal(err)
			}

			f.commit()
			if _, err := f.sync(false); err == nil {
				t.Fatal("unsafe symlink was accepted")
			}

			if !os.SameFile(before, statSyncFile(t, installed)) || !bytes.Equal(lockBefore, readSyncFile(t, filepath.Join(f.root, ".skillctl.lock"))) {
				t.Fatal("failed generation changed installed output or lock metadata")
			}
		})
	}
}
