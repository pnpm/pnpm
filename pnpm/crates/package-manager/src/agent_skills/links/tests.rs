use std::collections::BTreeSet;

#[cfg(unix)]
#[test]
fn only_a_link_into_a_package_skills_directory_is_a_package_skill() {
    let temp = tempfile::tempdir().unwrap();
    let claude_skills = temp.path().join(".claude/skills");
    std::fs::create_dir_all(&claude_skills).unwrap();
    for (target, expected) in [
        ("../node_modules/foo/skills/guide", true),
        ("../node_modules/@scope/foo/skills/engineering/guide", true),
        ("../node_modules/.pnpm/foo@1.0.0/node_modules/foo/skills/guide", true),
        ("../../packages/foo/skills/guide", true),
        ("../../packages/foo/skills/engineering/guide", true),
        ("../../packages/foo/skills/skills", true),
        ("../../packages/foo/skills/engineering/skills", true),
        ("../../.my-skills/skills/guide", true),
        ("../../../.external-skills/skills/guide", true),
        ("../../skills/guide", true),
        ("../../skills/skills", true),
        ("../../.claude/skills/guide", false),
        ("../../.agents/skills/guide", false),
        ("../node_modules/foo/docs/skills/guide", false),
        ("../skills/node_modules/foo/guide", false),
        ("../node_modules/foo/skills", false),
        ("../../packages/foo/docs/guide", false),
        ("../../packages/foo/skills", false),
        ("../../packages/foo/skills/.hidden", false),
        ("../../packages/foo/skills/a/b/c/d", false),
        ("../skills/guide", false),
        ("skills/guide", false),
    ] {
        let link = claude_skills.join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert_eq!(super::links_to_a_package_skill(temp.path(), &link), expected, "{target}");
    }

    let top_level_skills = temp.path().join("ai-skills");
    std::fs::create_dir_all(&top_level_skills).unwrap();
    for (target, expected) in [("../skills/guide", true), ("skills/guide", false)] {
        let link = top_level_skills.join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert_eq!(
            super::links_to_a_package_skill(temp.path(), &link),
            expected,
            "top-level dir target {target}",
        );
    }
}

#[test]
fn prune_removes_skills_named_skills_dot_prefixed_packages_and_top_level_agent_dirs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(root.join(".claude/skills")).unwrap();
    std::fs::create_dir_all(root.join("ai-skills")).unwrap();

    let pkg_skill_named_skills = root.join("packages/foo/skills/skills");
    let pkg_nested_skills = root.join("packages/foo/skills/engineering/skills");
    let dot_pkg_skill = root.join(".my-skills/skills/guide");
    let ext_dot_pkg_skill = temp.path().join(".ext-skills/skills/ext-guide");
    let root_pkg_skill = root.join("skills/root-guide");
    for dir in [
        &pkg_skill_named_skills,
        &pkg_nested_skills,
        &dot_pkg_skill,
        &ext_dot_pkg_skill,
        &root_pkg_skill,
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }

    let entries = [
        (".claude/skills/skills", &pkg_skill_named_skills),
        (".claude/skills/nested-skills", &pkg_nested_skills),
        (".claude/skills/guide", &dot_pkg_skill),
        (".claude/skills/ext-guide", &ext_dot_pkg_skill),
        ("ai-skills/root-guide", &root_pkg_skill),
    ];
    let mut recorded = Vec::new();
    for (entry, target) in entries {
        pnpm_fs::force_symlink_dir(target, &root.join(entry)).unwrap();
        recorded.push(entry.to_string());
    }

    super::prune(&root, &recorded, &BTreeSet::new()).unwrap();

    for (entry, _) in entries {
        assert!(
            std::fs::symlink_metadata(root.join(entry)).is_err(),
            "expected {entry} to be pruned",
        );
    }
}
