use anyhow::{Context, Result};
use serde_json::json;
use skillctl::generation::{render_template, route_guides, set_skill_name, write_router};
use skillctl::manifest::Router;
use std::fs;
use std::path::Path;

fn write(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    fs::create_dir_all(path.parent().context("generated test file has no parent")?)?;
    fs::write(path, content)?;
    Ok(())
}

#[test]
fn skill_name_only_changes_first_frontmatter_name() -> Result<()> {
    for (original, expected) in [
        (
            "---\nname: review\ndescription: d\n---\nbody\nname: keep\n",
            "---\nname: unique\ndescription: d\n---\nbody\nname: keep\n",
        ),
        (
            "---\ndescription: d\n---\nbody\n",
            "---\nname: unique\ndescription: d\n---\nbody\n",
        ),
        ("---\n---\nbody\n", "---\nname: unique\n---\nbody\n"),
        ("# body\n", "---\nname: unique\n---\n# body\n"),
        (
            "---\nname: old\n---  \nbody",
            "---\nname: unique\n---  \nbody",
        ),
        (
            "---\nname: old\nname: second\n---\n",
            "---\nname: unique\nname: second\n---\n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("SKILL.md"), original)?;
        set_skill_name(dir.path(), "unique").unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("SKILL.md")).unwrap(),
            expected
        );
        set_skill_name(dir.path(), "unique").unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("SKILL.md")).unwrap(),
            expected
        );
    }

    Ok(())
}

#[test]
fn router_matches_description_escaping_and_markdown_bytes() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("skills/beta/GUIDE.md"),
        "---\ndescription: [broken\n---\n",
    )?;
    write(
        &dir.path().join("skills/alpha/GUIDE.md"),
        "---\ndescription: >\n  Read quoted \"things\": then\n  use <tags> & café.\n---\n",
    )?;
    let members = vec!["beta".to_owned(), "alpha".to_owned()];
    write_router(
        dir.path(),
        &Router {
            name: "vendor".into(),
            description: "Use \"quotes\": <tags> & café 😀\nnext".into(),
        },
        &members,
    )
    .unwrap();
    let expected = concat!(
        "---\nname: vendor\ndescription: \"Use \\\"quotes\\\": <tags> & caf\\u00e9 \\ud83d\\ude00\\nnext\"\n---\n\n# vendor\n\n",
        "This skill indexes 2 skills. Pick the one that matches the task, read its guide, and follow it exactly as if it had been loaded as a skill. Resolve relative paths in a guide from that guide's directory.\n\n",
        "- `alpha` (`skills/alpha/GUIDE.md`): Read quoted \"things\": then use <tags> & café.\n",
        "- `beta` (`skills/beta/GUIDE.md`)\n",
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("SKILL.md")).unwrap(),
        expected
    );
    assert_eq!(members, ["beta", "alpha"]);

    Ok(())
}

#[test]
fn router_indexes_have_bounded_member_counts_and_character_threshold() -> Result<()> {
    for (count, description, index_count) in [
        (12, "short".to_owned(), 0),
        (13, "short".to_owned(), 2),
        (
            25,
            "Distinct routing detail ".to_owned() + &"word ".repeat(500),
            3,
        ),
        (
            1,
            "é".repeat(8000 - "- `task-00` (`skills/task-00/GUIDE.md`)".len()),
            0,
        ),
        (
            1,
            "é".repeat(8001 - "- `task-00` (`skills/task-00/GUIDE.md`)".len()),
            1,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let members: Vec<_> = (0..count).map(|index| format!("task-{index:02}")).collect();
        for name in &members {
            write(
                &dir.path().join("skills").join(name).join("GUIDE.md"),
                format!("---\ndescription: {description}\n---\n"),
            )?;
        }

        write_router(
            dir.path(),
            &Router {
                name: "vendor".into(),
                description: "Use for work.".into(),
            },
            &members,
        )
        .unwrap();
        let indexes = if dir.path().join("indexes").exists() {
            fs::read_dir(dir.path().join("indexes"))
                .unwrap()
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        assert_eq!(indexes.len(), index_count);
        let entry = fs::read_to_string(dir.path().join("SKILL.md")).unwrap();
        for index in indexes {
            let index = index.unwrap();
            let text = fs::read_to_string(index.path()).unwrap();
            assert!(text.matches("- `task-").count() <= 12);
            assert!(text.contains("`../skills/"));
            assert!(entry.contains(&format!("indexes/{}", index.file_name().to_string_lossy())));
            assert!(text.ends_with('\n'));
        }
    }

    Ok(())
}

#[test]
fn route_guides_rewrites_relative_local_references_only_and_preserves_binary_bytes() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let original = "Read [nested](nested/SKILL.md#topic) and `nested/SKILL.md`.\nKeep https://example.com/SKILL.md and /absolute/SKILL.md and missing/SKILL.md and mailto:SKILL.md.\n";
    write(&dir.path().join("SKILL.md"), original)?;
    write(
        &dir.path().join("nested/SKILL.md"),
        "[parent](../SKILL.md)\n",
    )?;
    write(
        &dir.path().join("nested/readme.md"),
        "`./SKILL.md` <../SKILL.md>\n",
    )?;
    let untouched = b"raw\r\n\xff\nhttps://example.com/SKILL.md\n";
    write(&dir.path().join("untouched.md"), untouched)?;
    write(&dir.path().join("data.txt"), "nested/SKILL.md\n")?;
    route_guides(dir.path()).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("GUIDE.md")).unwrap(),
        original.replace("nested/SKILL.md", "nested/GUIDE.md")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("nested/GUIDE.md")).unwrap(),
        "[parent](../GUIDE.md)\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("nested/readme.md")).unwrap(),
        "`./GUIDE.md` <../GUIDE.md>\n"
    );
    assert_eq!(
        fs::read(dir.path().join("untouched.md")).unwrap(),
        untouched
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("data.txt")).unwrap(),
        "nested/SKILL.md\n"
    );
    assert!(!dir.path().join("SKILL.md").exists());
    assert!(!dir.path().join("nested/SKILL.md").exists());

    Ok(())
}

#[test]
fn collision_preserves_entire_tree_before_first_write() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    let files = [
        ("SKILL.md", "nested/SKILL.md"),
        ("nested/SKILL.md", "../SKILL.md"),
        ("nested/GUIDE.md", "existing"),
    ];
    for (path, content) in files {
        write(&dir.path().join(path), content)?;
    }

    assert!(
        route_guides(dir.path())
            .unwrap_err()
            .to_string()
            .contains("guide already exists")
    );
    for (path, content) in files {
        assert_eq!(fs::read_to_string(dir.path().join(path)).unwrap(), content);
    }

    Ok(())
}

#[test]
fn strict_template_undefined_values_fail() -> Result<()> {
    assert!(render_template("router.md.j2", json!({"router":{"name":"vendor"}})).is_err());
    assert!(
        render_template(
            "router.md.j2",
            json!({"router":{"name":"vendor"},"entries":[],"indexes":[]})
        )
        .is_err()
    );
    assert!(render_template("missing", json!({})).is_err());

    Ok(())
}

#[test]
fn markdown_generation_preserves_non_utf8_body_and_header_bytes() -> Result<()> {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("SKILL.md"),
        b"---\nname: old\ndescription: \xff\n---\nbody\xff\n",
    )?;
    set_skill_name(dir.path(), "unique").unwrap();
    assert_eq!(
        fs::read(dir.path().join("SKILL.md")).unwrap(),
        b"---\nname: unique\ndescription: \xff\n---\nbody\xff\n"
    );
    write(
        &dir.path().join("skills/alpha/GUIDE.md"),
        b"---\ndescription: Read the guide.\n---\nbody\xff\n",
    )?;
    write_router(
        dir.path(),
        &Router {
            name: "vendor".into(),
            description: "Use for work.".into(),
        },
        &["alpha".into()],
    )
    .unwrap();
    assert!(
        fs::read_to_string(dir.path().join("SKILL.md"))
            .unwrap()
            .contains(": Read the guide.\n")
    );

    Ok(())
}

#[test]
fn lock_template_preserves_structured_json_and_escapes_descriptions() -> Result<()> {
    let data = json!({
        "version": 1,
        "dependencies": [{"id":"vendor", "skills":[{"description":"café 😀 <tags> & \"quotes\""}], "nullable":null, "enabled":true}],
        "state": {"tree":"abc", "names":["alpha","beta"], "generation":2}
    });
    let rendered = render_template("lock.json.j2", data.clone()).unwrap();
    let actual: serde_json::Value = serde_json::from_slice(&rendered).unwrap();
    assert_eq!(actual, data);
    let text = String::from_utf8(rendered).unwrap();
    assert!(text.contains("caf\\u00e9 \\ud83d\\ude00 <tags> & \\\"quotes\\\""));

    Ok(())
}
