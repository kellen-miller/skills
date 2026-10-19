package skillctl

import (
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func writeGenerationFixture(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}

	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}

func readGenerationFixture(t *testing.T, path string) string {
	t.Helper()
	content, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	return string(content)
}

func TestSetSkillName(t *testing.T) {
	cases := []struct{ original, expected string }{
		{"---\nname: review\ndescription: d\n---\nbody\nname: keep\n", "---\nname: unique\ndescription: d\n---\nbody\nname: keep\n"},
		{"---\ndescription: d\n---\nbody\n", "---\nname: unique\ndescription: d\n---\nbody\n"},
		{"---\n---\nbody\n", "---\nname: unique\n---\nbody\n"},
		{"# body\n", "---\nname: unique\n---\n# body\n"},
		{"---\nname: old\n---  \nbody", "---\nname: unique\n---  \nbody"},
		{"---\nname: old\nname: second\n---\n", "---\nname: unique\nname: second\n---\n"},
	}
	for i, test := range cases {
		t.Run(fmt.Sprint(i), func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "SKILL.md")
			writeGenerationFixture(t, path, test.original)
			if err := setSkillName(dir, "unique"); err != nil {
				t.Fatal(err)
			}

			if actual := readGenerationFixture(t, path); actual != test.expected {
				t.Fatalf("got %q, want %q", actual, test.expected)
			}
		})
	}
}

func TestWriteRouterOutput(t *testing.T) {
	dir := t.TempDir()
	writeGenerationFixture(t, filepath.Join(dir, "skills/beta/GUIDE.md"), "---\ndescription: [broken\n---\n")
	writeGenerationFixture(t, filepath.Join(dir, "skills/alpha/GUIDE.md"), "---\ndescription: >\n  Read quoted \"things\": then\n  use <tags> & café.\n---\n")
	members := []string{"beta", "alpha"}
	if err := writeRouter(dir, Router{Name: "vendor", Description: "Use \"quotes\": <tags> & café 😀\nnext"}, members); err != nil {
		t.Fatal(err)
	}

	expected := "---\nname: vendor\ndescription: \"Use \\\"quotes\\\": <tags> & caf\\u00e9 \\ud83d\\ude00\\nnext\"\n---\n\n# vendor\n\n" +
		"This skill indexes 2 skills. Pick the one that matches the task, read its guide, and follow it exactly as if it had been loaded as a skill. Resolve relative paths in a guide from that guide's directory.\n\n" +
		"- `alpha` (`skills/alpha/GUIDE.md`): Read quoted \"things\": then use <tags> & café.\n" +
		"- `beta` (`skills/beta/GUIDE.md`)\n"
	if actual := readGenerationFixture(t, filepath.Join(dir, "SKILL.md")); actual != expected {
		t.Fatalf("got %q, want %q", actual, expected)
	}

	if !reflect.DeepEqual(members, []string{"beta", "alpha"}) {
		t.Fatal("mutated member order")
	}
}

func TestRouterBoundedIndexes(t *testing.T) {
	for _, test := range []struct {
		name        string
		count       int
		description string
		indexCount  int
	}{
		{"twelve", 12, "short", 0},
		{"thirteen", 13, "short", 2},
		{"twenty-five", 25, "Distinct routing detail " + strings.Repeat("word ", 500), 3},
		{"exact-size", 1, strings.Repeat("é", 8000-len("- `task-00` (`skills/task-00/GUIDE.md`)")), 0},
		{"oversize", 1, strings.Repeat("é", 8001-len("- `task-00` (`skills/task-00/GUIDE.md`)")), 1},
	} {
		t.Run(test.name, func(t *testing.T) {
			dir := t.TempDir()
			var members []string
			for i := 0; i < test.count; i++ {
				member := fmt.Sprintf("task-%02d", i)
				members = append(members, member)
				writeGenerationFixture(t, filepath.Join(dir, "skills", member, "GUIDE.md"), "---\ndescription: "+test.description+"\n---\n")
			}

			if err := writeRouter(dir, Router{Name: "vendor", Description: "Use for work."}, members); err != nil {
				t.Fatal(err)
			}

			indexes, err := filepath.Glob(filepath.Join(dir, "indexes", "*.md"))
			if err != nil || len(indexes) != test.indexCount {
				t.Fatalf("indexes %v; error %v; want %d", indexes, err, test.indexCount)
			}

			entry := readGenerationFixture(t, filepath.Join(dir, "SKILL.md"))
			if test.indexCount == 0 {
				return
			}

			if strings.Contains(entry, test.description) {
				t.Fatal("large router includes descriptions")
			}

			for _, index := range indexes {
				text := readGenerationFixture(t, index)
				if strings.Count(text, "- `task-") > 12 || !strings.Contains(text, "`../skills/") || !strings.Contains(entry, "indexes/"+filepath.Base(index)) {
					t.Fatalf("bad index %s: %s", index, text)
				}
			}

			first := readGenerationFixture(t, indexes[0])
			last := members[min(11, len(members)-1)]
			if !strings.HasPrefix(first, "# vendor: task-00 – "+last+"\n\n- `task-00` (`../skills/task-00/GUIDE.md`): ") || !strings.HasSuffix(first, "\n") {
				t.Fatalf("unexpected index layout %q", first)
			}
		})
	}
}

func TestRouteGuides(t *testing.T) {
	dir := t.TempDir()
	original := "Read [nested](nested/SKILL.md#topic) and `nested/SKILL.md`.\nKeep https://example.com/SKILL.md and /absolute/SKILL.md and missing/SKILL.md.\n"
	writeGenerationFixture(t, filepath.Join(dir, "SKILL.md"), original)
	writeGenerationFixture(t, filepath.Join(dir, "nested/SKILL.md"), "[parent](../SKILL.md)\n")
	writeGenerationFixture(t, filepath.Join(dir, "nested/readme.md"), "`./SKILL.md` <../SKILL.md>\n")
	unchanged := "raw\r\n\xff\nhttps://example.com/SKILL.md\n"
	writeGenerationFixture(t, filepath.Join(dir, "untouched.md"), unchanged)
	writeGenerationFixture(t, filepath.Join(dir, "data.txt"), "nested/SKILL.md\n")
	if err := routeGuides(dir); err != nil {
		t.Fatal(err)
	}

	expected := strings.ReplaceAll(original, "nested/SKILL.md", "nested/GUIDE.md")
	for path, expected := range map[string]string{
		"GUIDE.md":         expected,
		"nested/GUIDE.md":  "[parent](../GUIDE.md)\n",
		"nested/readme.md": "`./GUIDE.md` <../GUIDE.md>\n",
		"untouched.md":     unchanged,
		"data.txt":         "nested/SKILL.md\n",
	} {
		if actual := readGenerationFixture(t, filepath.Join(dir, path)); actual != expected {
			t.Errorf("%s: got %q, want %q", path, actual, expected)
		}
	}

	for _, path := range []string{"SKILL.md", "nested/SKILL.md"} {
		if _, err := os.Stat(filepath.Join(dir, path)); !os.IsNotExist(err) {
			t.Errorf("entry point remains: %s (%v)", path, err)
		}
	}
}

func TestRouteGuideCollisionPreservesTree(t *testing.T) {
	dir := t.TempDir()
	for path, content := range map[string]string{"SKILL.md": "nested/SKILL.md", "nested/SKILL.md": "../SKILL.md", "nested/GUIDE.md": "existing"} {
		writeGenerationFixture(t, filepath.Join(dir, path), content)
	}

	if err := routeGuides(dir); err == nil || !strings.Contains(err.Error(), "guide already exists") {
		t.Fatalf("expected collision, got %v", err)
	}

	for path, expected := range map[string]string{"SKILL.md": "nested/SKILL.md", "nested/SKILL.md": "../SKILL.md", "nested/GUIDE.md": "existing"} {
		if actual := readGenerationFixture(t, filepath.Join(dir, path)); actual != expected {
			t.Errorf("collision changed %s: %q", path, actual)
		}
	}
}

func TestRenderTemplateFailureReturnsNoBytes(t *testing.T) {
	data := struct{ Router any }{Router: struct{ Name, Description any }{Name: "vendor", Description: make(chan int)}}
	content, err := renderTemplate("router.md.tmpl", data)
	if err == nil || content != nil {
		t.Fatalf("got bytes %q and error %v", content, err)
	}
}
