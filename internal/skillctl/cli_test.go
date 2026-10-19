package skillctl

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestCLIHelpAndErrors(t *testing.T) {
	for _, args := range [][]string{{"--help"}, {"sync", "--help"}, {"install-claude", "--help"}} {
		var out, stderr bytes.Buffer
		if code := Run(context.Background(), args, &out, &stderr); code != 0 {
			t.Fatalf("help %v: %d %s", args, code, stderr.String())
		}
	}

	for _, args := range [][]string{nil, {"unknown"}, {"sync", "--unknown"}, {"install-claude", "--no-claude"}, {"sync", "extra"}} {
		var out, stderr bytes.Buffer
		if code := Run(context.Background(), args, &out, &stderr); code != 2 {
			t.Fatalf("bad arguments %v: code=%d", args, code)
		}
	}
}

func TestCLIRootAndInstalledFallback(t *testing.T) {
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "deps.yaml"), []byte("version: 1\ndependencies: []\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	t.Setenv("XDG_CACHE_HOME", t.TempDir())
	var out, stderr bytes.Buffer
	args := []string{"sync", "--root", root, "--no-claude"}
	if code := Run(context.Background(), args, &out, &stderr); code != 0 {
		t.Fatalf("explicit root: %d %s", code, stderr.String())
	}

	if _, err := os.Stat(filepath.Join(root, "_managed")); err != nil {
		t.Fatal(err)
	}

	home := t.TempDir()
	if err := os.Mkdir(filepath.Join(home, ".agents"), 0o755); err != nil {
		t.Fatal(err)
	}

	if err := os.Symlink(root, filepath.Join(home, ".agents", "skills")); err != nil {
		t.Fatal(err)
	}

	t.Chdir(t.TempDir())
	actual, err := discoverRoot("", home)
	expected, resolveErr := filepath.EvalSymlinks(root)
	if err != nil || resolveErr != nil || actual != expected {
		t.Fatalf("installed fallback: %q %v", actual, err)
	}

	if _, err := discoverRoot("", t.TempDir()); err == nil {
		t.Fatal("missing checkout should fail")
	}
}
