package skillctl

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

func runGit(ctx context.Context, cwd string, args ...string) (string, error) {
	command := exec.CommandContext(ctx, "git", args...)
	command.Dir = cwd
	var stderr strings.Builder
	command.Stderr = &stderr
	output, err := command.Output()
	if err != nil {
		return "", fmt.Errorf("git %s: %w: %s", args[0], err, strings.TrimSpace(stderr.String()))
	}

	return strings.TrimSpace(string(output)), nil
}

func gitCacheRoot() (string, error) {
	base := os.Getenv("XDG_CACHE_HOME")
	if !filepath.IsAbs(base) {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}

		base = filepath.Join(home, ".cache")
	}

	return filepath.Join(base, "skillctl"), nil
}

func acquireDependency(ctx context.Context, dependency Dependency, cacheRoot string) (string, error) {
	identity, err := json.Marshal([]any{dependency.ID, dependency.URL, dependency.Ref, dependency.Sparse})
	if err != nil {
		return "", err
	}

	key := sha256.Sum256(identity)
	clone := filepath.Join(cacheRoot, "repos", hex.EncodeToString(key[:]))
	if _, err := os.Stat(clone); err == nil {
		gitDir, gitErr := runGit(ctx, clone, "rev-parse", "--absolute-git-dir")
		actual, actualErr := filepath.EvalSymlinks(gitDir)
		expected, expectedErr := filepath.EvalSymlinks(filepath.Join(clone, ".git"))
		origin, originErr := runGit(ctx, clone, "config", "--get", "remote.origin.url")
		if gitErr != nil || actualErr != nil || expectedErr != nil || actual != expected || originErr != nil || origin != dependency.URL {
			if err := os.RemoveAll(clone); err != nil {
				return "", err
			}
		}
	} else if !os.IsNotExist(err) {
		return "", err
	}

	if _, err := os.Stat(clone); os.IsNotExist(err) {
		if err := os.MkdirAll(clone, 0o755); err != nil {
			return "", err
		}

		if _, err := runGit(ctx, "", "init", "--quiet", clone); err != nil {
			return "", err
		}

		if _, err := runGit(ctx, clone, "remote", "add", "origin", dependency.URL); err != nil {
			return "", err
		}
	}

	ref := dependency.Ref
	if ref == "" {
		ref = "HEAD"
	}

	commands := [][]string{{"fetch", "--depth", "1", "--filter=blob:none", "--no-tags", "--force", "origin", ref}}
	if len(dependency.Sparse) != 0 {
		commands = append(commands, append([]string{"sparse-checkout", "set"}, dependency.Sparse...))
	}

	commands = append(commands, []string{"checkout", "--force", "--detach", "FETCH_HEAD"}, []string{"clean", "-ffdx"})
	for _, args := range commands {
		if _, err := runGit(ctx, clone, args...); err != nil {
			// Failed refreshes are never allowed to use stale cached contents.
			os.RemoveAll(clone)
			return "", fmt.Errorf("%s: %w", dependency.ID, err)
		}
	}

	return clone, nil
}
