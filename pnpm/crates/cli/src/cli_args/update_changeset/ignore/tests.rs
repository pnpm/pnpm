use super::IgnoredProjects;

#[test]
fn ignored_projects_matches_names_globs_and_normalized_dirs() {
    let ignored = IgnoredProjects::new(
        &["@scope/legacy-*".to_string()],
        &[
            "ignored-by-name".to_string(),
            "./packages/ignored-posix/".to_string(),
            r"./packages\ignored-windows\".to_string(),
        ],
    );

    assert!(ignored.matches("@scope/legacy-pkg", "packages/legacy"));
    assert!(ignored.matches("ignored-by-name", "packages/any"));
    assert!(ignored.matches("pkg-a", "packages/ignored-posix"));
    assert!(ignored.matches("pkg-b", "packages/ignored-windows"));
    assert!(!ignored.matches("pkg-c", "packages/kept"));
}
