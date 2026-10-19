package skillctl

import (
	"bytes"
	"embed"
	"encoding/json"
	"fmt"
	"io/fs"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"text/template"
	"unicode/utf16"
	"unicode/utf8"

	"go.yaml.in/yaml/v3"
)

//go:embed templates/*.tmpl
var outputTemplates embed.FS

var frontmatter = regexp.MustCompile(`(?sm)\A---\n(.*?)^---[ \t]*$`)
var nameLine = regexp.MustCompile(`(?m)^name:.*$`)
var guideReference = regexp.MustCompile("[^\\s`()<>\"']*SKILL\\.md")

// renderTemplate completes rendering before callers can publish any bytes.
func renderTemplate(name string, data any) ([]byte, error) {
	functions := template.FuncMap{
		"json": func(value any) (string, error) {
			var encoded bytes.Buffer
			encoder := json.NewEncoder(&encoded)
			encoder.SetEscapeHTML(false)
			if err := encoder.Encode(value); err != nil {
				return "", err
			}

			// Match Python's ensure_ascii output, including surrogate pairs.
			var result strings.Builder
			for _, char := range strings.TrimSuffix(encoded.String(), "\n") {
				if char < 0x7f {
					result.WriteRune(char)
				} else if char <= 0xffff {
					fmt.Fprintf(&result, `\u%04x`, char)
				} else {
					hi, lo := utf16.EncodeRune(char)
					fmt.Fprintf(&result, `\u%04x\u%04x`, hi, lo)
				}
			}

			return result.String(), nil
		},
		"join": strings.Join,
	}

	tmpl, err := template.New(name).Funcs(functions).Option("missingkey=error").ParseFS(outputTemplates, "templates/"+name)
	if err != nil {
		return nil, err
	}

	var rendered bytes.Buffer
	if err := tmpl.Execute(&rendered, data); err != nil {
		return nil, err
	}

	return rendered.Bytes(), nil
}

func setSkillName(dir, name string) error {
	path := filepath.Join(dir, "SKILL.md")
	content, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	text := string(content)
	line := "name: " + name
	match := frontmatter.FindStringSubmatchIndex(text)
	if match == nil {
		text = "---\n" + line + "\n---\n" + text
	} else {
		header := text[match[2]:match[3]]
		if existing := nameLine.FindStringIndex(header); existing != nil {
			header = header[:existing[0]] + line + header[existing[1]:]
		} else {
			header = line + "\n" + header
		}

		text = "---\n" + header + text[match[3]:]
	}

	if text == string(content) {
		return nil
	}

	return os.WriteFile(path, []byte(text), 0o644)
}

type routerEntry struct {
	Name    string
	Summary string
}

type routerIndex struct {
	Filename string
	Names    []string
	Entries  []routerEntry
}

func writeRouter(dir string, router Router, members []string) error {
	names := append([]string(nil), members...)
	sort.Strings(names)
	entries := make([]routerEntry, 0, len(names))
	size := 0
	for _, name := range names {
		content, err := os.ReadFile(filepath.Join(dir, "skills", name, "GUIDE.md"))
		if err != nil {
			return err
		}

		var summary string
		if match := frontmatter.FindSubmatch(content); match != nil {
			var metadata map[string]any
			if yaml.Unmarshal(match[1], &metadata) == nil {
				if description := metadata["description"]; description != nil && description != false && description != "" && description != 0 {
					summary = strings.Join(strings.Fields(fmt.Sprint(description)), " ")
				}
			}
		}

		entries = append(entries, routerEntry{Name: name, Summary: summary})
		size += utf8.RuneCountInString(fmt.Sprintf("- `%s` (`skills/%s/GUIDE.md`)", name, name)) + utf8.RuneCountInString(summary)
	}

	var indexes []routerIndex
	if len(entries) > 12 || size > 8000 {
		for offset := 0; offset < len(entries); offset += 12 {
			group := entries[offset:min(offset+12, len(entries))]
			index := routerIndex{Filename: group[0].Name + "--" + group[len(group)-1].Name + ".md", Entries: group}
			for _, entry := range group {
				index.Names = append(index.Names, "`"+entry.Name+"`")
			}

			indexes = append(indexes, index)
		}
	}

	data := struct {
		Router  Router
		Entries []routerEntry
		Indexes []routerIndex
	}{router, entries, indexes}

	content, err := renderTemplate("router.md.tmpl", data)
	if err != nil {
		return err
	}

	// Render every index before beginning filesystem writes.
	indexContents := make([][]byte, len(indexes))
	for i, index := range indexes {
		indexContents[i], err = renderTemplate("index.md.tmpl", struct {
			Router  Router
			First   string
			Last    string
			Entries []routerEntry
		}{router, index.Entries[0].Name, index.Entries[len(index.Entries)-1].Name, index.Entries})
		if err != nil {
			return err
		}
	}

	if len(indexes) > 0 {
		if err := os.MkdirAll(filepath.Join(dir, "indexes"), 0o755); err != nil {
			return err
		}

		for i, index := range indexes {
			if err := os.WriteFile(filepath.Join(dir, "indexes", index.Filename), indexContents[i], 0o644); err != nil {
				return err
			}
		}
	}

	return os.WriteFile(filepath.Join(dir, "SKILL.md"), content, 0o644)
}

func routeGuides(dir string) error {
	var skills, documents []string
	renamed := make(map[string]bool)
	err := filepath.WalkDir(dir, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}

		if entry.IsDir() {
			return nil
		}

		if strings.HasSuffix(entry.Name(), ".md") {
			documents = append(documents, path)
		}

		if entry.Name() != "SKILL.md" {
			return nil
		}

		if _, err := os.Lstat(filepath.Join(filepath.Dir(path), "GUIDE.md")); err == nil {
			return fmt.Errorf("routed guide already exists: %s", path)
		} else if !os.IsNotExist(err) {
			return err
		}

		resolved, err := filepath.EvalSymlinks(path)
		if err != nil {
			return err
		}

		absolute, err := filepath.Abs(resolved)
		if err != nil {
			return err
		}

		renamed[absolute] = true
		skills = append(skills, path)
		return nil
	})
	if err != nil {
		return err
	}

	for _, document := range documents {
		content, err := os.ReadFile(document)
		if err != nil {
			return err
		}

		rewritten := guideReference.ReplaceAllStringFunc(string(content), func(target string) string {
			parsed, parseErr := url.Parse(target)
			if filepath.IsAbs(target) || strings.Contains(target, "://") || (parseErr == nil && parsed.IsAbs()) {
				return target
			}

			resolved, err := filepath.EvalSymlinks(filepath.Join(filepath.Dir(document), target))
			if err != nil {
				return target
			}

			absolute, err := filepath.Abs(resolved)
			if err != nil || !renamed[absolute] {
				return target
			}

			return strings.TrimSuffix(target, "SKILL.md") + "GUIDE.md"
		})
		if rewritten != string(content) {
			if err := os.WriteFile(document, []byte(rewritten), 0o644); err != nil {
				return err
			}
		}
	}

	for _, path := range skills {
		if err := os.Rename(path, filepath.Join(filepath.Dir(path), "GUIDE.md")); err != nil {
			return err
		}
	}

	return nil
}
