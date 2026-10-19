package skillctl

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"syscall"
)

type resolvedDependency struct {
	ID     string `json:"id"`
	URL    string `json:"url"`
	Ref    string `json:"ref"`
	Commit string `json:"commit"`
}

type dependencyInput struct {
	Config Dependency `json:"config"`
	Trees  []string   `json:"trees"`
}

type generationInputs struct {
	Generator    string            `json:"generator"`
	DurableRoots []string          `json:"durable_roots"`
	Dependencies []dependencyInput `json:"dependencies"`
}

type generationState struct {
	Inputs generationInputs `json:"inputs"`
	Output string           `json:"output"`
}

type syncRecord struct {
	Version      int                  `json:"version"`
	Dependencies []resolvedDependency `json:"dependencies"`
	State        generationState      `json:"state"`
}

func syncLock(root string) (*os.File, error) {
	file, err := os.OpenFile(filepath.Join(root, ".skillctl.lock"), os.O_RDWR|os.O_CREATE, 0o600)
	if err != nil {
		return nil, err
	}

	if err := syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		file.Close()
		if errors.Is(err, syscall.EWOULDBLOCK) {
			return nil, errors.New("another sync is in progress")
		}

		return nil, err
	}

	return file, nil
}

func releaseLock(file *os.File) {
	// Close releases flock only after the final metadata write is complete.
	file.Close()
}

func renderSyncLock(record syncRecord) ([]byte, error) {
	content, err := renderTemplate("lock.json.tmpl", record)
	if err != nil {
		return nil, err
	}

	if !json.Valid(content) {
		return nil, fmt.Errorf("lock template produced invalid JSON")
	}

	return content, nil
}

func writeSyncLock(file *os.File, content []byte) error {
	if _, err := file.Seek(0, 0); err != nil {
		return err
	}

	if _, err := file.Write(content); err != nil {
		return err
	}

	// Keep the same inode so a second process cannot bypass the held flock.
	if err := file.Truncate(int64(len(content))); err != nil {
		return err
	}

	return file.Sync()
}

func recordsEqual(left, right any) bool {
	leftJSON, leftErr := json.Marshal(left)
	rightJSON, rightErr := json.Marshal(right)
	return leftErr == nil && rightErr == nil && bytes.Equal(leftJSON, rightJSON)
}
