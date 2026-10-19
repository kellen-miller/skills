package skillctl

import (
	"bytes"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"go.yaml.in/yaml/v3"
)

type Mapping struct {
	Source      string `json:"source" yaml:"source"`
	Destination string `json:"destination" yaml:"destination"`
}

type Selection struct {
	Mode    string    `json:"mode" yaml:"mode"`
	Include []Mapping `json:"include,omitempty" yaml:"include,omitempty"`
	Root    string    `json:"root,omitempty" yaml:"root,omitempty"`
	Exclude []string  `json:"exclude,omitempty" yaml:"exclude,omitempty"`
}

type Router struct {
	Name        string `json:"name" yaml:"name"`
	Description string `json:"description" yaml:"description"`
}

type Dependency struct {
	ID        string    `json:"id" yaml:"id"`
	URL       string    `json:"url" yaml:"url"`
	Ref       string    `json:"ref" yaml:"ref"`
	Sparse    []string  `json:"sparse" yaml:"sparse"`
	Selection Selection `json:"selection" yaml:"selection"`
	Claude    bool      `json:"claude" yaml:"claude"`
	Router    *Router   `json:"router" yaml:"router"`
}

type Manifest struct {
	Dependencies []Dependency `json:"dependencies" yaml:"dependencies"`
	DurableRoots []string     `json:"durable_roots" yaml:"durable_roots"`
}

// LoadManifest validates the complete public manifest and local overlay before
// callers acquire dependencies or mutate managed output.
func LoadManifest(root string) (Manifest, error) {
	base, err := readManifestMapping(filepath.Join(root, "deps.yaml"), []string{"version", "dependencies"}, []string{"version", "dependencies"})
	if err != nil {
		return Manifest{}, err
	}

	if !manifestVersionOne(base["version"]) {
		return Manifest{}, fmt.Errorf("deps.yaml version must be 1")
	}

	rawDependencies, ok := base["dependencies"].([]any)
	if !ok {
		return Manifest{}, fmt.Errorf("deps.yaml dependencies must be a list")
	}

	manifest := Manifest{Dependencies: make([]Dependency, 0, len(rawDependencies)), DurableRoots: []string{}}
	baseIDs := make(map[string]bool, len(rawDependencies))
	for index, raw := range rawDependencies {
		dependency, err := normalizeDependency(raw, fmt.Sprintf("deps.yaml.dependencies[%d]", index), true)
		if err != nil {
			return Manifest{}, err
		}

		manifest.Dependencies = append(manifest.Dependencies, dependency)
	}

	for _, dependency := range manifest.Dependencies {
		if baseIDs[dependency.ID] {
			return Manifest{}, fmt.Errorf("deps.yaml contains duplicate dependency IDs")
		}

		baseIDs[dependency.ID] = true
	}

	overlayPath := filepath.Join(root, "deps.local.yaml")
	_, statErr := os.Stat(overlayPath)
	if statErr != nil && !os.IsNotExist(statErr) {
		return Manifest{}, fmt.Errorf("cannot read deps.local.yaml: %w", statErr)
	}

	if statErr == nil {
		overlay, err := readManifestMapping(overlayPath, []string{"version", "add", "extend", "remove"}, []string{"version"})
		if err != nil {
			return Manifest{}, err
		}

		if !manifestVersionOne(overlay["version"]) {
			return Manifest{}, fmt.Errorf("deps.local.yaml version must be 1")
		}

		additions, extensions, removals := []any{}, []any{}, []any{}
		if value, exists := overlay["add"]; exists {
			additions, ok = value.([]any)
			if !ok {
				return Manifest{}, fmt.Errorf("deps.local.yaml add and extend must be lists")
			}
		}

		if value, exists := overlay["extend"]; exists {
			extensions, ok = value.([]any)
			if !ok {
				return Manifest{}, fmt.Errorf("deps.local.yaml add and extend must be lists")
			}
		}

		if value, exists := overlay["remove"]; exists {
			removals, ok = value.([]any)
			if !ok {
				return Manifest{}, fmt.Errorf("deps.local.yaml remove must be a list")
			}
		}

		byID := make(map[string]int, len(manifest.Dependencies)+len(additions))
		for index, dependency := range manifest.Dependencies {
			byID[dependency.ID] = index
		}

		for index, raw := range additions {
			addition, err := normalizeDependency(raw, fmt.Sprintf("deps.local.yaml.add[%d]", index), false)
			if err != nil {
				return Manifest{}, err
			}

			if _, exists := byID[addition.ID]; exists {
				return Manifest{}, fmt.Errorf("deps.local.yaml add redefines dependency %s", addition.ID)
			}

			byID[addition.ID] = len(manifest.Dependencies)
			manifest.Dependencies = append(manifest.Dependencies, addition)
		}

		for index, raw := range extensions {
			context := fmt.Sprintf("deps.local.yaml.extend[%d]", index)
			extension, err := manifestMapping(raw, context, []string{"id", "include"}, []string{"id", "include"})
			if err != nil {
				return Manifest{}, err
			}

			id, err := safeName(extension["id"], context+".id")
			if err != nil {
				return Manifest{}, err
			}

			dependencyIndex, exists := byID[id]
			if !exists {
				return Manifest{}, fmt.Errorf("deps.local.yaml extend names unknown dependency %s", id)
			}

			dependency := &manifest.Dependencies[dependencyIndex]
			if dependency.Selection.Mode != "explicit" {
				return Manifest{}, fmt.Errorf("deps.local.yaml cannot extend discovery dependency %s", id)
			}

			extra, err := normalizeInclude(extension["include"], context+".include")
			if err != nil {
				return Manifest{}, err
			}

			for _, mapping := range extra {
				if len(dependency.Sparse) > 0 && !beneathSparse(mapping.Source, dependency.Sparse) {
					return Manifest{}, fmt.Errorf("deps.local.yaml source %s is outside %s sparse paths", mapping.Source, id)
				}
			}

			existing := make(map[Mapping]bool, len(dependency.Selection.Include))
			for _, mapping := range dependency.Selection.Include {
				existing[mapping] = true
			}

			for _, mapping := range extra {
				if existing[mapping] {
					return Manifest{}, fmt.Errorf("deps.local.yaml repeats a mapping for %s", id)
				}
			}

			dependency.Selection.Include = append(dependency.Selection.Include, extra...)
		}

		removeIDs := make(map[string]bool, len(removals))
		for index, raw := range removals {
			id, err := safeName(raw, fmt.Sprintf("deps.local.yaml.remove[%d]", index))
			if err != nil {
				return Manifest{}, err
			}

			if !baseIDs[id] {
				return Manifest{}, fmt.Errorf("deps.local.yaml remove names unknown base dependency %s", id)
			}

			removeIDs[id] = true
		}

		if len(removeIDs) > 0 {
			retained := make([]Dependency, 0, len(manifest.Dependencies))
			for _, dependency := range manifest.Dependencies {
				if !removeIDs[dependency.ID] {
					retained = append(retained, dependency)
				}
			}

			manifest.Dependencies = retained
		}
	}

	entries, err := os.ReadDir(root)
	if err != nil {
		return Manifest{}, fmt.Errorf("cannot read authored skills: %w", err)
	}

	durable := make(map[string]bool)
	for _, entry := range entries {
		if entry.Name() == "_managed" {
			continue
		}

		// Stat follows symlinks, matching Path.is_dir/is_file in the original CLI.
		directory, err := os.Stat(filepath.Join(root, entry.Name()))
		if err != nil || !directory.IsDir() {
			continue
		}

		skill, err := os.Stat(filepath.Join(root, entry.Name(), "SKILL.md"))
		if err == nil && skill.Mode().IsRegular() {
			durable[entry.Name()] = true
			manifest.DurableRoots = append(manifest.DurableRoots, entry.Name())
		}
	}

	sort.Strings(manifest.DurableRoots)
	owners := make(map[string]string)
	for _, dependency := range manifest.Dependencies {
		for _, destination := range topLevelNames(dependency) {
			if durable[destination] {
				return Manifest{}, fmt.Errorf("managed destination %s collides with an authored skill", destination)
			}

			if owner, exists := owners[destination]; exists {
				return Manifest{}, fmt.Errorf("duplicate destination %s in %s and %s", destination, owner, dependency.ID)
			}

			owners[destination] = dependency.ID
		}
	}

	return manifest, nil
}

func readManifestMapping(path string, allowed, required []string) (map[string]any, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("cannot read %s: %w", filepath.Base(path), err)
	}

	// Preserve PyYAML's legacy booleans before decoding untyped values; typed
	// string decoding would silently coerce numbers and booleans into names.
	decoder := yaml.NewDecoder(bytes.NewReader(data))
	var document yaml.Node
	if err := decoder.Decode(&document); err != nil && err != io.EOF {
		return nil, fmt.Errorf("cannot read %s: %w", filepath.Base(path), err)
	}

	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		if err != nil {
			return nil, fmt.Errorf("cannot read %s: %w", filepath.Base(path), err)
		}

		return nil, fmt.Errorf("cannot read %s: expected a single document", filepath.Base(path))
	}

	normalizeLegacyYAMLBooleans(&document)
	var raw any
	if err := document.Decode(&raw); err != nil {
		return nil, fmt.Errorf("cannot read %s: %w", filepath.Base(path), err)
	}

	return manifestMapping(raw, filepath.Base(path), allowed, required)
}

func normalizeLegacyYAMLBooleans(node *yaml.Node) {
	if node.Kind == yaml.ScalarNode && node.Tag == "!!str" && node.Style == 0 {
		switch node.Value {
		case "yes", "Yes", "YES", "on", "On", "ON":
			node.Tag, node.Value = "!!bool", "true"
		case "no", "No", "NO", "off", "Off", "OFF":
			node.Tag, node.Value = "!!bool", "false"
		}
	}

	// Anchored scalars live in Content; aliases point at those same nodes.
	for _, child := range node.Content {
		normalizeLegacyYAMLBooleans(child)
	}
}

func manifestMapping(raw any, context string, allowed, required []string) (map[string]any, error) {
	value, ok := raw.(map[string]any)
	if !ok {
		return nil, fmt.Errorf("%s must be a mapping", context)
	}

	unknown, missing := []string{}, []string{}
	for key := range value {
		found := false
		for _, allowedKey := range allowed {
			if key == allowedKey {
				found = true
				break
			}
		}

		if !found {
			unknown = append(unknown, key)
		}
	}

	for _, key := range required {
		if _, exists := value[key]; !exists {
			missing = append(missing, key)
		}
	}

	if len(unknown) > 0 {
		sort.Strings(unknown)
		return nil, fmt.Errorf("%s has unknown keys: %s", context, strings.Join(unknown, ", "))
	}

	if len(missing) > 0 {
		sort.Strings(missing)
		return nil, fmt.Errorf("%s is missing keys: %s", context, strings.Join(missing, ", "))
	}

	return value, nil
}

func manifestVersionOne(value any) bool {
	switch version := value.(type) {
	case int:
		return version == 1
	case float64:
		return version == 1
	case bool:
		return version // Python's numeric version comparison accepts true.
	default:
		return false
	}
}

func safeRepoPath(raw any, context string, allowDot bool) (string, error) {
	value, ok := raw.(string)
	if !ok || value == "" || strings.ContainsAny(value, "\x00\\") {
		return "", fmt.Errorf("%s must be a non-empty POSIX path", context)
	}

	if strings.HasPrefix(value, "/") {
		return "", fmt.Errorf("%s must stay inside the repository", context)
	}

	for _, part := range strings.Split(value, "/") {
		if part == "" || part == ".." {
			return "", fmt.Errorf("%s must stay inside the repository", context)
		}
	}

	if value == "." {
		if allowDot {
			return value, nil
		}

		return "", fmt.Errorf("%s cannot be .", context)
	}

	for _, part := range strings.Split(value, "/") {
		if part == "." {
			return "", fmt.Errorf("%s must be normalized", context)
		}
	}

	return value, nil
}

func safeName(raw any, context string) (string, error) {
	value, ok := raw.(string)
	if !ok || value == "" || value == "_managed" || strings.HasPrefix(value, ".") || strings.ContainsAny(value, "/\\\x00") {
		return "", fmt.Errorf("%s must be a safe directory name", context)
	}

	return value, nil
}

func uniqueManifestStrings(raw any, context string, paths bool) ([]string, error) {
	values, ok := raw.([]any)
	if !ok {
		return nil, fmt.Errorf("%s must be a list of strings", context)
	}

	for _, value := range values {
		if _, ok := value.(string); !ok {
			return nil, fmt.Errorf("%s must be a list of strings", context)
		}
	}

	result := make([]string, 0, len(values))
	seen := make(map[string]bool, len(values))
	for _, value := range values {
		var normalized string
		var err error
		if paths {
			normalized, err = safeRepoPath(value, context+" entry", false)
		} else {
			normalized, err = safeName(value, context+" entry")
		}

		if err != nil {
			return nil, err
		}

		if seen[normalized] {
			return nil, fmt.Errorf("%s contains duplicates", context)
		}

		seen[normalized] = true
		result = append(result, normalized)
	}

	return result, nil
}

func beneathSparse(path string, prefixes []string) bool {
	for _, prefix := range prefixes {
		if prefix == "." || path == prefix || strings.HasPrefix(path, prefix+"/") {
			return true
		}
	}

	return false
}

func normalizeInclude(raw any, context string) ([]Mapping, error) {
	values, ok := raw.([]any)
	if !ok || len(values) == 0 {
		return nil, fmt.Errorf("%s must be a non-empty list", context)
	}

	result := make([]Mapping, 0, len(values))
	pairs, destinations := make(map[Mapping]bool), make(map[string]bool)
	for index, raw := range values {
		itemContext := fmt.Sprintf("%s[%d]", context, index)
		item, err := manifestMapping(raw, itemContext, []string{"source", "destination"}, []string{"source", "destination"})
		if err != nil {
			return nil, err
		}

		source, err := safeRepoPath(item["source"], itemContext+".source", true)
		if err != nil {
			return nil, err
		}

		destination, err := safeName(item["destination"], itemContext+".destination")
		if err != nil {
			return nil, err
		}

		mapping := Mapping{Source: source, Destination: destination}
		pairs[mapping], destinations[destination] = true, true
		result = append(result, mapping)
	}

	if len(pairs) != len(result) {
		return nil, fmt.Errorf("%s contains duplicate mappings", context)
	}

	if len(destinations) != len(result) {
		return nil, fmt.Errorf("%s contains duplicate destinations", context)
	}

	return result, nil
}

func normalizeDependency(raw any, context string, base bool) (Dependency, error) {
	item, err := manifestMapping(raw, context, []string{"id", "url", "ref", "sparse", "selection", "claude", "router"}, []string{"id", "url", "selection"})
	if err != nil {
		return Dependency{}, err
	}

	dependency := Dependency{Claude: true}
	dependency.ID, err = safeName(item["id"], context+".id")
	if err != nil {
		return Dependency{}, err
	}

	var ok bool
	dependency.URL, ok = item["url"].(string)
	if !ok {
		return Dependency{}, fmt.Errorf("%s.url must be a string", context)
	}

	if base {
		if !strings.HasPrefix(dependency.URL, "https://github.com/") || !strings.HasSuffix(dependency.URL, ".git") {
			return Dependency{}, fmt.Errorf("%s.url must be an HTTPS GitHub URL", context)
		}
	} else if !strings.HasPrefix(dependency.URL, "https://") && !strings.HasPrefix(dependency.URL, "ssh://") && !strings.HasPrefix(dependency.URL, "git"+"@") {
		return Dependency{}, fmt.Errorf("%s.url must use HTTPS or SSH", context)
	}

	if ref := item["ref"]; ref != nil {
		dependency.Ref, ok = ref.(string)
		if !ok || dependency.Ref == "" {
			return Dependency{}, fmt.Errorf("%s.ref must be a non-empty string", context)
		}
	}

	if sparse := item["sparse"]; sparse != nil {
		dependency.Sparse, err = uniqueManifestStrings(sparse, context+".sparse", true)
		if err != nil {
			return Dependency{}, err
		}

		if len(dependency.Sparse) == 0 {
			return Dependency{}, fmt.Errorf("%s.sparse must not be empty", context)
		}
	}

	if claude, exists := item["claude"]; exists {
		dependency.Claude, ok = claude.(bool)
		if !ok {
			return Dependency{}, fmt.Errorf("%s.claude must be a boolean", context)
		}
	}

	if rawRouter := item["router"]; rawRouter != nil {
		router, err := manifestMapping(rawRouter, context+".router", []string{"name", "description"}, []string{"name", "description"})
		if err != nil {
			return Dependency{}, err
		}

		description, ok := router["description"].(string)
		if !ok || strings.TrimSpace(description) == "" {
			return Dependency{}, fmt.Errorf("%s.router.description must be a string", context)
		}

		name, err := safeName(router["name"], context+".router.name")
		if err != nil {
			return Dependency{}, err
		}

		dependency.Router = &Router{Name: name, Description: strings.Join(strings.Fields(description), " ")}
	}

	selection, ok := item["selection"].(map[string]any)
	if !ok {
		return Dependency{}, fmt.Errorf("%s.selection must be a mapping", context)
	}

	mode, _ := selection["mode"].(string)
	dependency.Selection.Mode = mode
	switch mode {
	case "explicit":
		if _, err := manifestMapping(selection, context+".selection", []string{"mode", "include"}, []string{"mode", "include"}); err != nil {
			return Dependency{}, err
		}

		dependency.Selection.Include, err = normalizeInclude(selection["include"], context+".selection.include")
		if err != nil {
			return Dependency{}, err
		}

		for _, mapping := range dependency.Selection.Include {
			if len(dependency.Sparse) > 0 && !beneathSparse(mapping.Source, dependency.Sparse) {
				return Dependency{}, fmt.Errorf("%s source %s is outside sparse paths", context, mapping.Source)
			}
		}

	case "discovery":
		if _, err := manifestMapping(selection, context+".selection", []string{"mode", "root", "exclude"}, []string{"mode", "root"}); err != nil {
			return Dependency{}, err
		}

		dependency.Selection.Root, err = safeRepoPath(selection["root"], context+".selection.root", true)
		if err != nil {
			return Dependency{}, err
		}

		exclude, exists := selection["exclude"]
		if !exists {
			exclude = []any{}
		}

		dependency.Selection.Exclude, err = uniqueManifestStrings(exclude, context+".selection.exclude", false)
		if err != nil {
			return Dependency{}, err
		}

		if len(dependency.Sparse) > 0 && !beneathSparse(dependency.Selection.Root, dependency.Sparse) {
			return Dependency{}, fmt.Errorf("%s discovery root %s is outside sparse paths", context, dependency.Selection.Root)
		}

	default:
		return Dependency{}, fmt.Errorf("%s.selection.mode must be explicit or discovery", context)
	}

	if !dependency.Claude && mode != "explicit" && dependency.Router == nil {
		return Dependency{}, fmt.Errorf("%s.claude: false requires explicit selection or a router", context)
	}

	return dependency, nil
}

func topLevelNames(dependency Dependency) []string {
	if dependency.Router != nil {
		return []string{dependency.Router.Name}
	}

	var names []string
	if dependency.Selection.Mode == "explicit" {
		for _, mapping := range dependency.Selection.Include {
			names = append(names, mapping.Destination)
		}
	}

	return names
}
