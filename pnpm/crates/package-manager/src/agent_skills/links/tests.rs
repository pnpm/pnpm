#[cfg(unix)]
#[test]
fn only_a_link_into_a_package_skills_directory_is_a_package_skill() {
    let temp = tempfile::tempdir().unwrap();
    for (target, expected) in [
        ("../node_modules/foo/skills/guide", true),
        ("../node_modules/@scope/foo/skills/engineering/guide", true),
        ("../node_modules/.pnpm/foo@1.0.0/node_modules/foo/skills/guide", true),
        ("../node_modules/foo/docs/skills/guide", false),
        ("../skills/node_modules/foo/guide", false),
        ("../elsewhere/skills/guide", false),
    ] {
        let link = temp.path().join("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert_eq!(super::links_to_a_package_skill(&link), expected, "{target}");
    }
}
