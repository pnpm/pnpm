#[cfg(unix)]
use super::is_shim_pointing_at;
use super::{
    Arc, Host, LinkBinsOptions, PackageBinSource, Path, Value, create_dir_all, json,
    link_bins_of_packages, read_file, read_to_string, tempdir, write_file,
};
#[cfg(unix)]
use std::fs::metadata;

#[cfg(unix)]
#[test]
fn link_node_bin_symlinks_directly_instead_of_writing_shim() {
    let tmp = tempdir().unwrap();
    let bin_target = tmp.path().join("bin_target");
    let node_dir = tmp.path().join("node_pkg");
    let node_bin_dir = node_dir.join("bin");
    create_dir_all(&node_bin_dir).unwrap();
    write_file(node_bin_dir.join("node"), "fake-node-binary").unwrap();
    write_file(
        node_dir.join("package.json"),
        json!({"name": "node", "version": "20.0.0", "bin": {"node": "bin/node"}}).to_string(),
    )
    .unwrap();

    let manifest: Value =
        serde_json::from_slice(&read_file(node_dir.join("package.json")).unwrap()).unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(node_dir, Arc::new(manifest))],
        &bin_target,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let bin_location = bin_target.join("node");
    let meta = std::fs::symlink_metadata(&bin_location).unwrap();
    assert!(meta.file_type().is_symlink(), "node bin must be a symlink, not a shim file");
    assert_eq!(
        std::fs::canonicalize(&bin_location).unwrap(),
        std::fs::canonicalize(node_bin_dir.join("node")).unwrap(),
        "symlink must resolve to the real binary",
    );
    assert_eq!(
        read_to_string(node_bin_dir.join("node")).unwrap(),
        "fake-node-binary",
        "node bin special case must not rewrite the underlying binary",
    );
}

#[cfg(unix)]
#[test]
fn link_node_bin_replaces_dangling_symlink() {
    use std::os::unix::fs::symlink;
    let tmp = tempdir().unwrap();
    let bin_target = tmp.path().join("bin_target");
    create_dir_all(&bin_target).unwrap();
    let dangling_target = tmp.path().join("does_not_exist");
    symlink(&dangling_target, bin_target.join("node")).unwrap();
    assert!(
        std::fs::metadata(bin_target.join("node")).is_err(),
        "precondition: symlink must be dangling",
    );

    let node_dir = tmp.path().join("node_pkg");
    let node_bin_dir = node_dir.join("bin");
    create_dir_all(&node_bin_dir).unwrap();
    write_file(node_bin_dir.join("node"), "fake-node-binary").unwrap();
    write_file(
        node_dir.join("package.json"),
        json!({"name": "node", "version": "20.0.0", "bin": {"node": "bin/node"}}).to_string(),
    )
    .unwrap();

    let manifest: Value =
        serde_json::from_slice(&read_file(node_dir.join("package.json")).unwrap()).unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(node_dir, Arc::new(manifest))],
        &bin_target,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let stat = std::fs::symlink_metadata(bin_target.join("node")).unwrap();
    assert!(stat.file_type().is_symlink());
    assert_eq!(
        std::fs::canonicalize(bin_target.join("node")).unwrap(),
        std::fs::canonicalize(node_bin_dir.join("node")).unwrap(),
    );
}

#[test]
fn prefer_symlinked_executables_links_bins_as_relative_symlinks() {
    let tmp = tempdir().unwrap();
    let pkg_dir = tmp.path().join("node_modules/foo");
    create_dir_all(&pkg_dir).unwrap();
    write_file(
        pkg_dir.join("package.json"),
        json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"}).to_string(),
    )
    .unwrap();
    write_file(pkg_dir.join("cli.js"), "#!/usr/bin/env node\nconsole.log('hello_world')\n")
        .unwrap();

    let bins_dir = tmp.path().join("node_modules/.bin");
    let manifest_value: Value =
        serde_json::from_slice(&read_file(pkg_dir.join("package.json")).unwrap()).unwrap();
    let options =
        LinkBinsOptions { prefer_symlinked_executables: true, ..LinkBinsOptions::default() };
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg_dir.clone(), Arc::new(manifest_value))],
        &bins_dir,
        &options,
    )
    .unwrap();

    let bin = bins_dir.join("foo");
    if cfg!(windows) {
        // The setting is inert on Windows: bins keep their shims.
        assert!(
            std::fs::symlink_metadata(&bin).unwrap().file_type().is_file(),
            "Windows must keep writing shims under preferSymlinkedExecutables",
        );
        return;
    }
    assert_eq!(
        std::fs::read_link(&bin).expect("bin must be a symlink"),
        Path::new("../foo/cli.js"),
        "the symlink must be relative, like pnpm's symlink-dir",
    );
    // The read follows the symlink: the entry resolves to the bin
    // source itself, not a shim body.
    assert!(read_to_string(&bin).unwrap().contains("hello_world"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            metadata(pkg_dir.join("cli.js")).unwrap().permissions().mode() & 0o777,
            0o755,
            "the target file gets the executable bits, like pnpm's ensureExecutable",
        );
    }
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
fn prefer_symlinked_executables_links_a_bin_whose_target_is_missing() {
    let tmp = tempdir().unwrap();
    let pkg_dir = tmp.path().join("node_modules/foo");
    create_dir_all(&pkg_dir).unwrap();
    write_file(
        pkg_dir.join("package.json"),
        json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"}).to_string(),
    )
    .unwrap();

    let bins_dir = tmp.path().join("node_modules/.bin");
    let manifest_value: Value =
        serde_json::from_slice(&read_file(pkg_dir.join("package.json")).unwrap()).unwrap();
    let options =
        LinkBinsOptions { prefer_symlinked_executables: true, ..LinkBinsOptions::default() };
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg_dir, Arc::new(manifest_value))],
        &bins_dir,
        &options,
    )
    .unwrap();

    // A dangling symlink is still created — pnpm warns and keeps it,
    // so a later step can materialize the target.
    assert_eq!(
        std::fs::read_link(bins_dir.join("foo")).expect("dangling bin symlink must exist"),
        Path::new("../foo/cli.js"),
    );
}

#[cfg(unix)]
#[test]
fn dangling_symlink_at_shim_path_is_replaced_with_a_shim() {
    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let bins_dir = tmp.path().join(".bin");
    create_dir_all(&bins_dir).unwrap();
    std::os::unix::fs::symlink(tmp.path().join("nowhere"), bins_dir.join("foo")).unwrap();

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg.clone(), Arc::new(manifest))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(bins_dir.join("foo")).expect("real shim replaces the dangling link");
    assert!(is_shim_pointing_at(&body, &pkg.join("cli.js")));
}

#[cfg(unix)]
#[test]
fn stale_shim_rewrite_replaces_a_symlink_instead_of_writing_through_it() {
    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let bins_dir = tmp.path().join(".bin");
    create_dir_all(&bins_dir).unwrap();
    let victim = tmp.path().join("victim");
    write_file(&victim, "precious").unwrap();
    std::os::unix::fs::symlink(&victim, bins_dir.join("foo")).unwrap();

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg.clone(), Arc::new(manifest))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    assert_eq!(read_to_string(&victim).unwrap(), "precious", "the symlink target is untouched");
    assert!(
        !std::fs::symlink_metadata(bins_dir.join("foo")).unwrap().file_type().is_symlink(),
        "the shim is a regular file",
    );
    let body = read_to_string(bins_dir.join("foo")).unwrap();
    assert!(is_shim_pointing_at(&body, &pkg.join("cli.js")));
}
