use super::local_child_specifier;

#[test]
fn local_child_specifiers_keep_resolver_paths_unambiguous() {
    let lockfile_dir = std::env::current_dir().unwrap().join("workspace");
    let importer_dir = lockfile_dir.join("packages/app");
    let absolute_path = lockfile_dir
        .join("child")
        .display()
        .to_string()
        .replace('\\', "/");

    assert_eq!(
        local_child_specifier("link:child", &lockfile_dir, &importer_dir),
        Some(format!("link:{absolute_path}")),
    );
    assert_eq!(
        local_child_specifier("./child", &lockfile_dir, &importer_dir),
        Some("../../child".to_string()),
    );
    assert_eq!(
        local_child_specifier("link:~/child", &lockfile_dir, &importer_dir),
        Some("link:~/child".to_string()),
    );
    assert_eq!(
        local_child_specifier(&format!("link:{absolute_path}"), &lockfile_dir, &importer_dir),
        Some(format!("link:{absolute_path}")),
    );
}

#[test]
fn local_child_specifiers_keep_unambiguous_local_path_shapes() {
    let lockfile_dir = std::env::current_dir().unwrap().join("workspace");
    assert_eq!(
        local_child_specifier("./child", &lockfile_dir, &lockfile_dir),
        Some("./child".to_string()),
    );
}
