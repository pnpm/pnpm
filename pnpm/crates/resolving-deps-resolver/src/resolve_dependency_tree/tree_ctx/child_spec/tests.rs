use super::relative_local_child_specifier;

#[test]
fn local_child_specifiers_are_rebased_to_the_importer() {
    let lockfile_dir = std::env::current_dir().unwrap().join("workspace");
    let importer_dir = lockfile_dir.join("packages/app");
    let absolute_spec = format!("link:{}", lockfile_dir.join("child").display());

    assert_eq!(
        relative_local_child_specifier("link:child", &lockfile_dir, &importer_dir),
        Some("link:../../child".to_string()),
    );
    assert_eq!(
        relative_local_child_specifier("./child", &lockfile_dir, &importer_dir),
        Some("../../child".to_string()),
    );
    assert_eq!(
        relative_local_child_specifier("link:~/child", &lockfile_dir, &importer_dir),
        Some("link:~/child".to_string()),
    );
    assert_eq!(
        relative_local_child_specifier(&absolute_spec, &lockfile_dir, &importer_dir),
        Some(absolute_spec),
    );
}

#[test]
fn local_child_specifiers_keep_unambiguous_local_path_shapes() {
    let lockfile_dir = std::env::current_dir().unwrap().join("workspace");
    let importer_dir = lockfile_dir.clone();

    assert_eq!(
        relative_local_child_specifier("./child", &lockfile_dir, &importer_dir),
        Some("./child".to_string()),
    );
}
