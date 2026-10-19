package skillctl

import (
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"go.yaml.in/yaml/v3"
)

func manifestTestDependency() map[string]any {
	return map[string]any{
		"id": "example", "url": "https://github.com/example/example.git",
		"selection": map[string]any{
			"mode":    "explicit",
			"include": []any{map[string]any{"source": "skills/alpha", "destination": "alpha"}},
		},
	}
}

func manifestTestRoot(t *testing.T, dependencies []any, overlay map[string]any, authored ...string) string {
	t.Helper()
	root := t.TempDir()
	files := map[string]any{"deps.yaml": map[string]any{"version": 1, "dependencies": dependencies}}
	if overlay != nil {
		files["deps.local.yaml"] = overlay
	}

	for name, value := range files {
		data, err := yaml.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}

		if err := os.WriteFile(filepath.Join(root, name), data, 0o600); err != nil {
			t.Fatal(err)
		}
	}

	for _, name := range authored {
		if err := os.MkdirAll(filepath.Join(root, name), 0o700); err != nil {
			t.Fatal(err)
		}

		if err := os.WriteFile(filepath.Join(root, name, "SKILL.md"), []byte("---\n"), 0o600); err != nil {
			t.Fatal(err)
		}
	}

	return root
}

func TestManifestLoadModesAndDefaults(t *testing.T) {
	explicit := manifestTestDependency()
	explicit["ref"], explicit["sparse"], explicit["router"] = nil, nil, nil
	discovery := map[string]any{
		"id": "discover", "url": "https://github.com/example/discover.git", "sparse": []any{"skills"},
		"selection": map[string]any{"mode": "discovery", "root": "skills", "exclude": []any{"skip"}},
	}
	root := manifestTestRoot(t, []any{explicit, discovery}, nil, "z-authored", "a-authored", "_managed")
	manifest, err := LoadManifest(root)
	if err != nil {
		t.Fatal(err)
	}

	if len(manifest.Dependencies) != 2 || !manifest.Dependencies[0].Claude || manifest.Dependencies[0].Ref != "" || manifest.Dependencies[0].Sparse != nil {
		t.Fatalf("unexpected dependencies: %+v", manifest.Dependencies)
	}

	if got := manifest.Dependencies[1].Selection; got.Mode != "discovery" || got.Root != "skills" || !reflect.DeepEqual(got.Exclude, []string{"skip"}) {
		t.Fatalf("unexpected discovery: %+v", got)
	}

	if !reflect.DeepEqual(manifest.DurableRoots, []string{"a-authored", "z-authored"}) {
		t.Fatalf("unexpected authored roots: %v", manifest.DurableRoots)
	}
}

func TestManifestRejectDependencyFields(t *testing.T) {
	cases := []struct {
		name   string
		change func(map[string]any)
		want   string
	}{
		{"unknown dependency", func(d map[string]any) { d["mystery"] = true }, "unknown keys"},
		{"missing id", func(d map[string]any) { delete(d, "id") }, "missing keys"},
		{"missing url", func(d map[string]any) { delete(d, "url") }, "missing keys"},
		{"missing selection", func(d map[string]any) { delete(d, "selection") }, "missing keys"},
		{"numeric id", func(d map[string]any) { d["id"] = 12 }, "safe directory name"},
		{"bool url", func(d map[string]any) { d["url"] = false }, "must be a string"},
		{"public SSH", func(d map[string]any) { d["url"] = "git" + "@example.test/repo.git" }, "HTTPS GitHub URL"},
		{"public other host", func(d map[string]any) { d["url"] = "https://example.test/repo.git" }, "HTTPS GitHub URL"},
		{"public no suffix", func(d map[string]any) { d["url"] = "https://github.com/example/repo" }, "HTTPS GitHub URL"},
		{"numeric ref", func(d map[string]any) { d["ref"] = 12 }, "non-empty string"},
		{"empty ref", func(d map[string]any) { d["ref"] = "" }, "non-empty string"},
		{"string claude", func(d map[string]any) { d["claude"] = "no" }, "must be a boolean"},
		{"null claude", func(d map[string]any) { d["claude"] = nil }, "must be a boolean"},
		{"numeric claude", func(d map[string]any) { d["claude"] = 1 }, "must be a boolean"},
		{"scalar sparse", func(d map[string]any) { d["sparse"] = "skills" }, "list of strings"},
		{"numeric sparse", func(d map[string]any) { d["sparse"] = []any{12} }, "list of strings"},
		{"empty sparse", func(d map[string]any) { d["sparse"] = []any{} }, "must not be empty"},
		{"duplicate sparse", func(d map[string]any) { d["sparse"] = []any{"skills", "skills"} }, "contains duplicates"},
		{"dot sparse", func(d map[string]any) { d["sparse"] = []any{"."} }, "cannot be ."},
		{"outside sparse", func(d map[string]any) { d["sparse"] = []any{"other"} }, "outside sparse paths"},
		{"sparse sibling", func(d map[string]any) { d["sparse"] = []any{"skill"} }, "outside sparse paths"},
		{"null selection", func(d map[string]any) { d["selection"] = nil }, "must be a mapping"},
		{"unknown selection", func(d map[string]any) { d["selection"].(map[string]any)["mystery"] = true }, "unknown keys"},
		{"wrong mode fields", func(d map[string]any) { d["selection"].(map[string]any)["root"] = "skills" }, "unknown keys"},
		{"missing include", func(d map[string]any) { delete(d["selection"].(map[string]any), "include") }, "missing keys"},
		{"invalid mode", func(d map[string]any) { d["selection"].(map[string]any)["mode"] = true }, "explicit or discovery"},
		{"empty include", func(d map[string]any) { d["selection"].(map[string]any)["include"] = []any{} }, "non-empty list"},
		{"null include", func(d map[string]any) { d["selection"].(map[string]any)["include"] = nil }, "non-empty list"},
		{"unknown mapping", func(d map[string]any) {
			d["selection"].(map[string]any)["include"].([]any)[0].(map[string]any)["mystery"] = true
		}, "unknown keys"},
		{"missing source", func(d map[string]any) {
			delete(d["selection"].(map[string]any)["include"].([]any)[0].(map[string]any), "source")
		}, "missing keys"},
		{"duplicate mapping", func(d map[string]any) {
			s := d["selection"].(map[string]any)
			s["include"] = append(s["include"].([]any), map[string]any{"source": "skills/alpha", "destination": "alpha"})
		}, "duplicate mappings"},
		{"duplicate destination", func(d map[string]any) {
			s := d["selection"].(map[string]any)
			s["include"] = append(s["include"].([]any), map[string]any{"source": "skills/beta", "destination": "alpha"})
		}, "duplicate destinations"},
		{"unknown router", func(d map[string]any) {
			d["router"] = map[string]any{"name": "vendor", "description": "Vendor", "x": true}
		}, "unknown keys"},
		{"missing router description", func(d map[string]any) { d["router"] = map[string]any{"name": "vendor"} }, "missing keys"},
		{"empty router description", func(d map[string]any) { d["router"] = map[string]any{"name": "vendor", "description": " \n "} }, "description must be a string"},
		{"numeric router description", func(d map[string]any) { d["router"] = map[string]any{"name": "vendor", "description": 12} }, "description must be a string"},
		{"unsafe router", func(d map[string]any) { d["router"] = map[string]any{"name": "../x", "description": "Vendor"} }, "safe directory name"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			dependency := manifestTestDependency()
			test.change(dependency)
			root := manifestTestRoot(t, []any{dependency}, nil, "_managed")
			marker := filepath.Join(root, "_managed", "keep.txt")
			if err := os.WriteFile(marker, []byte("unchanged"), 0o600); err != nil {
				t.Fatal(err)
			}

			if _, err := LoadManifest(root); err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("got %v; want %q", err, test.want)
			}

			if data, err := os.ReadFile(marker); err != nil || string(data) != "unchanged" {
				t.Fatalf("managed output changed: %q, %v", data, err)
			}
		})
	}
}

func TestManifestUnsafePathsAndNames(t *testing.T) {
	for _, source := range []any{"/absolute", "../escape", "a\\b", "a/./b", "", "a//b", "a/", "a/../b", "a\x00b", 12, false} {
		t.Run("source/"+fmtManifestTestValue(source), func(t *testing.T) {
			d := manifestTestDependency()
			d["selection"].(map[string]any)["include"].([]any)[0].(map[string]any)["source"] = source
			if _, err := LoadManifest(manifestTestRoot(t, []any{d}, nil)); err == nil {
				t.Fatalf("accepted unsafe source %v", source)
			}
		})
	}

	for _, name := range []any{"../escape", "nested/name", "nested\\name", ".hidden", "..", ".", "_managed", "", "a\x00b", 12, false} {
		for _, field := range []string{"id", "destination"} {
			t.Run(field+"/"+fmtManifestTestValue(name), func(t *testing.T) {
				d := manifestTestDependency()
				if field == "id" {
					d["id"] = name
				} else {
					d["selection"].(map[string]any)["include"].([]any)[0].(map[string]any)[field] = name
				}

				if _, err := LoadManifest(manifestTestRoot(t, []any{d}, nil)); err == nil {
					t.Fatalf("accepted unsafe %s %v", field, name)
				}
			})
		}
	}
}

func fmtManifestTestValue(value any) string {
	data, _ := yaml.Marshal(value)
	return strings.TrimSpace(string(data))
}

func TestManifestDiscoveryValidation(t *testing.T) {
	cases := []struct {
		name      string
		selection map[string]any
		fields    map[string]any
		want      string
	}{
		{"unknown", map[string]any{"mode": "discovery", "root": "skills", "include": []any{}}, nil, "unknown keys"},
		{"missing root", map[string]any{"mode": "discovery"}, nil, "missing keys"},
		{"null exclude", map[string]any{"mode": "discovery", "root": "skills", "exclude": nil}, nil, "list of strings"},
		{"duplicate exclude", map[string]any{"mode": "discovery", "root": "skills", "exclude": []any{"a", "a"}}, nil, "contains duplicates"},
		{"numeric root", map[string]any{"mode": "discovery", "root": 12}, nil, "POSIX path"},
		{"outside sparse", map[string]any{"mode": "discovery", "root": "skills"}, map[string]any{"sparse": []any{"other"}}, "outside sparse paths"},
		{"claude false", map[string]any{"mode": "discovery", "root": "skills"}, map[string]any{"claude": false}, "requires explicit selection or a router"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			d := manifestTestDependency()
			d["selection"] = test.selection
			for key, value := range test.fields {
				d[key] = value
			}

			if _, err := LoadManifest(manifestTestRoot(t, []any{d}, nil)); err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("got %v; want %q", err, test.want)
			}
		})
	}

	d := manifestTestDependency()
	d["selection"] = map[string]any{"mode": "discovery", "root": "."}
	d["claude"] = false
	d["router"] = map[string]any{"name": "vendor", "description": "Use for  vendor\n work."}
	manifest, err := LoadManifest(manifestTestRoot(t, []any{d}, nil))
	if err != nil {
		t.Fatal(err)
	}

	if got := manifest.Dependencies[0]; got.Router.Description != "Use for vendor work." || got.Claude || got.Selection.Exclude == nil {
		t.Fatalf("unexpected normalization: %+v", got)
	}
}

func TestManifestOverlayAddExtendRemove(t *testing.T) {
	local := manifestTestDependency()
	local["id"], local["url"] = "local", "git"+"@example.test:local.git"
	local["selection"] = map[string]any{"mode": "explicit", "include": []any{map[string]any{"source": ".", "destination": "local"}}}
	second := manifestTestDependency()
	second["id"] = "removed"
	overlay := map[string]any{
		"version": 1, "add": []any{local}, "remove": []any{"removed", "removed"},
		"extend": []any{map[string]any{"id": "example", "include": []any{map[string]any{"source": "skills/beta", "destination": "beta"}}}},
	}
	manifest, err := LoadManifest(manifestTestRoot(t, []any{manifestTestDependency(), second}, overlay))
	if err != nil {
		t.Fatal(err)
	}

	if len(manifest.Dependencies) != 2 || manifest.Dependencies[0].ID != "example" || manifest.Dependencies[1].ID != "local" || !reflect.DeepEqual(topLevelNames(manifest.Dependencies[0]), []string{"alpha", "beta"}) {
		t.Fatalf("unexpected overlay result: %+v", manifest)
	}

	// Local additions also allow HTTPS non-GitHub and ssh:// addresses.
	for _, url := range []string{"https://private.test/repo", "ssh://source@example.test/local.git"} {
		local["url"] = url
		if _, err := LoadManifest(manifestTestRoot(t, []any{}, map[string]any{"version": 1, "add": []any{local}})); err != nil {
			t.Fatalf("local URL %q: %v", url, err)
		}
	}
}

func TestManifestOverlayValidation(t *testing.T) {
	extra := []any{map[string]any{"source": "skills/beta", "destination": "beta"}}
	cases := []struct {
		name    string
		overlay map[string]any
		want    string
	}{
		{"unknown", map[string]any{"version": 1, "x": true}, "unknown keys"},
		{"missing version", map[string]any{}, "missing keys"},
		{"wrong version", map[string]any{"version": 2}, "version must be 1"},
		{"null add", map[string]any{"version": 1, "add": nil}, "add and extend must be lists"},
		{"null extend", map[string]any{"version": 1, "extend": nil}, "add and extend must be lists"},
		{"null remove", map[string]any{"version": 1, "remove": nil}, "remove must be a list"},
		{"redefine", map[string]any{"version": 1, "add": []any{manifestTestDependency()}}, "redefines dependency"},
		{"unknown extension", map[string]any{"version": 1, "extend": []any{map[string]any{"id": "missing", "include": extra}}}, "unknown dependency"},
		{"override extension", map[string]any{"version": 1, "extend": []any{map[string]any{"id": "example", "url": "https://example.test/x", "include": extra}}}, "unknown keys"},
		{"missing extension field", map[string]any{"version": 1, "extend": []any{map[string]any{"id": "example"}}}, "missing keys"},
		{"repeat mapping", map[string]any{"version": 1, "extend": []any{map[string]any{"id": "example", "include": []any{map[string]any{"source": "skills/alpha", "destination": "alpha"}}}}}, "repeats a mapping"},
		{"repeat destination", map[string]any{"version": 1, "extend": []any{map[string]any{"id": "example", "include": []any{map[string]any{"source": "skills/beta", "destination": "alpha"}}}}}, "duplicate destination"},
		{"remove unknown", map[string]any{"version": 1, "remove": []any{"missing"}}, "unknown base dependency"},
		{"unsafe remove", map[string]any{"version": 1, "remove": []any{"../escape"}}, "safe directory name"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			if _, err := LoadManifest(manifestTestRoot(t, []any{manifestTestDependency()}, test.overlay)); err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("got %v; want %q", err, test.want)
			}
		})
	}

	local := manifestTestDependency()
	local["id"] = "local"
	if _, err := LoadManifest(manifestTestRoot(t, []any{}, map[string]any{"version": 1, "add": []any{local}, "remove": []any{"local"}})); err == nil || !strings.Contains(err.Error(), "unknown base dependency") {
		t.Fatalf("local removal: %v", err)
	}

	discovery := manifestTestDependency()
	discovery["selection"] = map[string]any{"mode": "discovery", "root": "."}
	extension := map[string]any{"version": 1, "extend": []any{map[string]any{"id": "example", "include": extra}}}
	if _, err := LoadManifest(manifestTestRoot(t, []any{discovery}, extension)); err == nil || !strings.Contains(err.Error(), "cannot extend discovery") {
		t.Fatalf("discovery extension: %v", err)
	}

	sparse := manifestTestDependency()
	sparse["sparse"] = []any{"skills/alpha"}
	if _, err := LoadManifest(manifestTestRoot(t, []any{sparse}, extension)); err == nil || !strings.Contains(err.Error(), "outside example sparse paths") {
		t.Fatalf("sparse extension: %v", err)
	}

	local["url"] = "file:///tmp/local"
	if _, err := LoadManifest(manifestTestRoot(t, []any{}, map[string]any{"version": 1, "add": []any{local}})); err == nil || !strings.Contains(err.Error(), "HTTPS or SSH") {
		t.Fatalf("local file URL: %v", err)
	}
}

func TestManifestDestinationOwnership(t *testing.T) {
	first, second := manifestTestDependency(), manifestTestDependency()
	if _, err := LoadManifest(manifestTestRoot(t, []any{first, second}, nil)); err == nil || !strings.Contains(err.Error(), "duplicate dependency IDs") {
		t.Fatalf("duplicate IDs: %v", err)
	}

	second["id"] = "second"
	if _, err := LoadManifest(manifestTestRoot(t, []any{first, second}, nil)); err == nil || !strings.Contains(err.Error(), "duplicate destination") {
		t.Fatalf("duplicate destinations: %v", err)
	}

	if _, err := LoadManifest(manifestTestRoot(t, []any{first}, nil, "alpha")); err == nil || !strings.Contains(err.Error(), "authored skill") {
		t.Fatalf("authored collision: %v", err)
	}

	first["router"] = map[string]any{"name": "vendor", "description": "Vendor"}
	if _, err := LoadManifest(manifestTestRoot(t, []any{first, second}, nil)); err != nil {
		t.Fatalf("router should isolate nested alpha: %v", err)
	}

	if _, err := LoadManifest(manifestTestRoot(t, []any{first}, nil, "vendor")); err == nil || !strings.Contains(err.Error(), "authored skill") {
		t.Fatalf("authored router collision: %v", err)
	}

	second["router"] = first["router"]
	if _, err := LoadManifest(manifestTestRoot(t, []any{first, second}, nil)); err == nil || !strings.Contains(err.Error(), "duplicate destination vendor") {
		t.Fatalf("duplicate routers: %v", err)
	}
}

func TestManifestYAMLDocumentValidation(t *testing.T) {
	cases := []struct{ source, want string }{
		{"", "must be a mapping"},
		{"[]", "must be a mapping"},
		{"version: 1\ndependencies: []\nunknown: true\n", "unknown keys"},
		{"version: 1\n", "missing keys"},
		{"version: '1'\ndependencies: []\n", "version must be 1"},
		{"version: 2\ndependencies: []\n", "version must be 1"},
		{"version: 1\ndependencies: null\n", "dependencies must be a list"},
		{"version: 1\ndependencies: [null]\n", "must be a mapping"},
		{"version: 1\ndependencies: []\n---\nversion: 1\ndependencies: []\n", "single document"},
		{"version: [\n", "cannot read deps.yaml"},
	}
	for _, test := range cases {
		t.Run(test.source, func(t *testing.T) {
			root := t.TempDir()
			if err := os.WriteFile(filepath.Join(root, "deps.yaml"), []byte(test.source), 0o600); err != nil {
				t.Fatal(err)
			}

			if _, err := LoadManifest(root); err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("got %v; want %q", err, test.want)
			}
		})
	}

	if _, err := LoadManifest(t.TempDir()); err == nil || !strings.Contains(err.Error(), "cannot read deps.yaml") {
		t.Fatalf("missing file: %v", err)
	}
}

func TestManifestLegacyYAMLBooleans(t *testing.T) {
	for _, token := range []string{"yes", "Yes", "YES", "on", "On", "ON", "no", "No", "NO", "off", "Off", "OFF", `"no"`, "'no'", "!!str no", "yEs"} {
		t.Run(token, func(t *testing.T) {
			root := t.TempDir()
			source := "version: 1\ndependencies:\n  - id: example\n    url: https://github.com/example/example.git\n    claude: " + token + "\n    selection:\n      mode: explicit\n      include:\n        - source: skills/alpha\n          destination: alpha\n"
			if err := os.WriteFile(filepath.Join(root, "deps.yaml"), []byte(source), 0o600); err != nil {
				t.Fatal(err)
			}

			manifest, err := LoadManifest(root)
			if strings.ContainsAny(token, "\"'!") || token == "yEs" {
				if err == nil || !strings.Contains(err.Error(), "must be a boolean") {
					t.Fatalf("quoted or explicit strings must stay strings: %v", err)
				}

				return
			}

			if err != nil {
				t.Fatal(err)
			}

			want := strings.EqualFold(token, "yes") || strings.EqualFold(token, "on")
			if got := manifest.Dependencies[0].Claude; got != want {
				t.Fatalf("claude: %s decoded %v; want %v", token, got, want)
			}
		})
	}

	for _, token := range []string{"no", "ON", "true", "123"} {
		t.Run("name/"+token, func(t *testing.T) {
			root := t.TempDir()
			source := "version: 1\ndependencies:\n  - id: " + token + "\n    url: https://github.com/example/example.git\n    selection: {mode: explicit, include: [{source: skill, destination: alpha}]}\n"
			if err := os.WriteFile(filepath.Join(root, "deps.yaml"), []byte(source), 0o600); err != nil {
				t.Fatal(err)
			}

			if _, err := LoadManifest(root); err == nil || !strings.Contains(err.Error(), "safe directory name") {
				t.Fatalf("boolean/numeric name accepted: %v", err)
			}
		})
	}

	root := t.TempDir()
	source := `version: 1
dependencies:
  - id: first
    url: https://github.com/example/example.git
    claude: &flag no
    selection: {mode: explicit, include: [{source: skill, destination: first}]}
  - id: second
    url: https://github.com/example/example.git
    claude: *flag
    selection: {mode: explicit, include: [{source: skill, destination: second}]}
`
	if err := os.WriteFile(filepath.Join(root, "deps.yaml"), []byte(source), 0o600); err != nil {
		t.Fatal(err)
	}

	manifest, err := LoadManifest(root)
	if err != nil {
		t.Fatal(err)
	}

	if manifest.Dependencies[0].Claude || manifest.Dependencies[1].Claude {
		t.Fatal("legacy boolean alias must resolve false")
	}
}
