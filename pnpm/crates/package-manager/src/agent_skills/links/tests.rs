#[cfg(unix)]
#[test]
fn only_a_link_into_a_package_skills_directory_is_a_package_skill() {
    let temp = tempfile::tempdir().unwrap();
    for (target, expected) in [
        ("../node_modules/foo/skills/guide", true),
        ("../node_modules/@scope/foo/skills/engineering/guide", true),
        ("../node_modules/.pnpm/foo@1.0.0/node_modules/foo/skills/guide", true),
        ("../../packages/foo/skills/guide", true),
        ("../../packages/foo/skills/engineering/guide", true),
        ("../../skills/guide", true),
        ("../elsewhere/skills/guide", true),
        ("../../.claude/skills/guide", false),
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
        let link = temp.path().join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert_eq!(super::links_to_a_package_skill(&link), expected, "{target}");
    }
}
