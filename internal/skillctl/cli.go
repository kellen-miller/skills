package skillctl

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// Run executes the installed CLI without depending on its build-time source path.
func Run(ctx context.Context, args []string, stdout, stderr io.Writer) int {
	usage := "usage: skillctl {sync,install-claude} [--dry-run] [--root PATH] [--claude-target PATH]\n"
	if len(args) == 0 {
		fmt.Fprint(stderr, usage)
		return 2
	}

	if args[0] == "--help" || args[0] == "-h" {
		fmt.Fprint(stdout, usage)
		return 0
	}

	command := args[0]
	if command != "sync" && command != "install-claude" {
		fmt.Fprint(stderr, usage)
		return 2
	}

	home, err := os.UserHomeDir()
	if err != nil {
		fmt.Fprintln(stderr, "ERROR:", err)
		return 1
	}

	flags := flag.NewFlagSet(command, flag.ContinueOnError)
	flags.SetOutput(stderr)
	dryRun := flags.Bool("dry-run", false, "preview changes without installing")
	rootFlag := flags.String("root", "", "skills checkout (default: current checkout or ~/.agents/skills)")
	claudeTarget := flags.String("claude-target", filepath.Join(home, ".claude", "skills"), "Claude skills directory")
	var noClaude bool
	if command == "sync" {
		flags.BoolVar(&noClaude, "no-claude", false, "skip the Claude Code install")
	}

	if err := flags.Parse(args[1:]); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return 0
		}

		return 2
	}

	if flags.NArg() != 0 {
		fmt.Fprintln(stderr, "ERROR: unexpected arguments:", strings.Join(flags.Args(), " "))
		return 2
	}

	root, err := discoverRoot(*rootFlag, home)
	if err == nil {
		var manifest Manifest
		manifest, err = LoadManifest(root)
		if err == nil && command == "sync" {
			err = Sync(ctx, root, manifest, *dryRun, stdout)
		}

		if err == nil && (command == "install-claude" || !noClaude) {
			err = InstallClaude(root, manifest, *claudeTarget, *dryRun, stdout)
		}
	}

	if err != nil {
		fmt.Fprintln(stderr, "ERROR:", err)
		return 1
	}

	return 0
}

func discoverRoot(explicit, home string) (string, error) {
	if explicit != "" {
		root, err := filepath.Abs(explicit)
		if err != nil {
			return "", err
		}

		return filepath.EvalSymlinks(root)
	}

	current, err := os.Getwd()
	if err != nil {
		return "", err
	}

	for path := current; ; path = filepath.Dir(path) {
		if isFile(filepath.Join(path, "deps.yaml")) {
			return filepath.EvalSymlinks(path)
		}

		if filepath.Dir(path) == path {
			break
		}
	}

	fallback := filepath.Join(home, ".agents", "skills")
	if isFile(filepath.Join(fallback, "deps.yaml")) {
		return filepath.EvalSymlinks(fallback)
	}

	return "", errors.New("cannot find skills checkout; pass --root or install ~/.agents/skills")
}
