package skillctl

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
)

func treeFingerprint(root string) (string, error) {
	info, err := os.Lstat(root)
	if os.IsNotExist(err) {
		return "", nil
	}

	if err != nil {
		return "", err
	}

	if !info.IsDir() {
		return "", nil
	}

	digest := sha256.New()
	err = filepath.WalkDir(root, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}

		if path == root {
			return nil
		}

		info, err := entry.Info()
		if err != nil {
			return err
		}

		var content []byte
		switch {
		case info.Mode()&os.ModeSymlink != 0:
			target, readErr := os.Readlink(path)
			err = readErr
			content = []byte(target)
		case info.Mode().IsRegular():
			content, err = os.ReadFile(path)
		case info.IsDir():
		default:
			return fmt.Errorf("unsupported skill file: %s", path)
		}

		if err != nil {
			return err
		}

		relative, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}

		header, err := json.Marshal([]any{filepath.ToSlash(relative), uint32(info.Mode()), len(content)})
		if err != nil {
			return err
		}

		digest.Write(header)
		digest.Write([]byte("\n"))
		digest.Write(content)
		return nil
	})
	if err != nil {
		return "", err
	}

	return hex.EncodeToString(digest.Sum(nil)), nil
}

func treesEqual(left, right string) (bool, error) {
	leftFingerprint, err := treeFingerprint(left)
	if err != nil || leftFingerprint == "" {
		return false, err
	}

	rightFingerprint, err := treeFingerprint(right)
	return err == nil && leftFingerprint == rightFingerprint, err
}

func containedPath(root, path string) bool {
	relative, err := filepath.Rel(root, path)
	return err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)) && !filepath.IsAbs(relative)
}

// copySkillTree follows repository-local links while rejecting escapes and cycles.
// An empty repositoryRoot preserves authored Claude skills' existing link behavior.
func copySkillTree(source, target, repositoryRoot string) error {
	if repositoryRoot != "" {
		resolved, err := filepath.EvalSymlinks(repositoryRoot)
		if err != nil {
			return err
		}

		repositoryRoot = resolved
	}

	ancestors := make(map[string]bool)
	var copyEntry func(string, string) error
	copyEntry = func(source, target string) error {
		resolved, err := filepath.EvalSymlinks(source)
		if err != nil {
			return err
		}

		if repositoryRoot != "" && !containedPath(repositoryRoot, resolved) {
			return fmt.Errorf("source symlink escapes repository: %s", source)
		}

		info, err := os.Stat(resolved)
		if err != nil {
			return err
		}

		if info.IsDir() {
			if ancestors[resolved] {
				return fmt.Errorf("source symlink cycle: %s", source)
			}

			ancestors[resolved] = true
			defer delete(ancestors, resolved)
			if err := os.Mkdir(target, 0o755); err != nil {
				return err
			}

			entries, err := os.ReadDir(source)
			if err != nil {
				return err
			}

			for _, entry := range entries {
				if entry.Name() == ".git" {
					continue
				}

				if err := copyEntry(filepath.Join(source, entry.Name()), filepath.Join(target, entry.Name())); err != nil {
					return err
				}
			}

			return os.Chmod(target, info.Mode().Perm())
		}

		if !info.Mode().IsRegular() {
			return fmt.Errorf("unsupported skill file: %s", source)
		}

		content, err := os.ReadFile(source)
		if err != nil {
			return err
		}

		return os.WriteFile(target, content, info.Mode().Perm())
	}

	return copyEntry(source, target)
}

func directoryNames(root string) ([]string, error) {
	entries, err := os.ReadDir(root)
	if os.IsNotExist(err) {
		return []string{}, nil
	}

	if err != nil {
		return nil, err
	}

	names := make([]string, 0, len(entries))
	for _, entry := range entries {
		names = append(names, entry.Name())
	}

	return names, nil
}

func isDirectory(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.IsDir()
}

func isFile(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.Mode().IsRegular()
}
