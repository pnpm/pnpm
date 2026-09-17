use super::{
    Arc, Host, LinkBinsOptions, PackageBinSource, Path, PathBuf, create_dir_all, json,
    link_bins_of_packages, read_to_string, tempdir, write_file,
};
use pnpm_fs::lexical_normalize;
use std::{fs, os::unix::fs::symlink, process::Command};

/// Link a package whose bin prints `NODE_PATH` into `<root>/node_modules/.bin`,
/// with three existing `NODE_PATH` entries: the package's own `node_modules`,
/// its slot's, and the hidden hoisted one.
fn link_node_path_printer(root: &Path, relocatable_root: Option<PathBuf>) -> PathBuf {
    let pkg_dir = root.join("node_modules/.pnpm/foo@1.0.0/node_modules/foo");
    let hoisted = root.join("node_modules/.pnpm/node_modules");
    create_dir_all(pkg_dir.join("node_modules")).unwrap();
    create_dir_all(&hoisted).unwrap();
    write_file(pkg_dir.join("print.sh"), "#!/bin/sh\nprintf '%s' \"$NODE_PATH\"\n").unwrap();
    let manifest = json!({"name": "foo", "version": "1.0.0", "bin": "print.sh"});
    let bin_dir = root.join("node_modules/.bin");
    let options = LinkBinsOptions {
        extra_node_paths: vec![hoisted.to_string_lossy().into_owned()],
        relocatable_root,
        ..LinkBinsOptions::default()
    };
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg_dir, Arc::new(manifest))],
        &bin_dir,
        &options,
    )
    .unwrap();
    bin_dir
}

#[test]
fn relocated_shim_resolves_node_path_from_its_new_location() {
    let tmp = tempdir().unwrap();
    let base = dunce::canonicalize(tmp.path()).unwrap();
    let old_root = base.join("old");
    link_node_path_printer(&old_root, Some(old_root.clone()));
    let root = base.join("new");
    fs::rename(&old_root, &root).unwrap();
    symlink(root.join("node_modules/.bin"), base.join("bin-link")).unwrap();
    // A `$basedir` a `cd` would look up in `$CDPATH` before the working
    // directory, and a decoy for it to find there.
    create_dir_all(base.join("node_modules/.bin")).unwrap();

    let mut by_absolute_path = Command::new(root.join("node_modules/.bin/foo"));
    by_absolute_path.current_dir(&base);
    let mut with_a_hostile_cdpath = Command::new("/bin/sh");
    with_a_hostile_cdpath
        .arg("node_modules/.bin/foo")
        .env("CDPATH", &base)
        .current_dir(&root);
    let mut through_a_dir_symlink = Command::new("/bin/sh");
    through_a_dir_symlink.arg("bin-link/foo").current_dir(&base);

    let invocations = [
        ("absolute $0", by_absolute_path),
        ("relative $0 with a hostile $CDPATH", with_a_hostile_cdpath),
        ("relative $0 through a directory symlink", through_a_dir_symlink),
    ];
    for (label, mut command) in invocations {
        let output = command
            .env_remove("NODE_PATH")
            .output()
            .unwrap();
        let node_path = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{label}: NODE_PATH={node_path}\nSTDERR:\n{stderr}");
        assert!(output.status.success(), "{label}");
        assert_eq!(node_path.split(':').count(), 3, "{label}");
        let outside: Vec<&str> = node_path
            .split(':')
            // Node collapses the `..` in a `NODE_PATH` entry lexically.
            .filter(|entry| {
                let resolved = lexical_normalize(Path::new(entry));
                !(resolved.starts_with(&root) && resolved.is_dir())
            })
            .collect();
        assert!(outside.is_empty(), "{label}: {outside:?} do not resolve inside the moved tree");
    }
}

#[test]
fn node_bin_symlink_is_relative_under_the_root() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("project");
    let node_dir = root.join("node_modules/node");
    create_dir_all(node_dir.join("bin")).unwrap();
    write_file(node_dir.join("bin/node"), "fake-node-binary").unwrap();
    let manifest = json!({"name": "node", "version": "20.0.0", "bin": {"node": "bin/node"}});
    let bin_dir = root.join("node_modules/.bin");
    let options =
        LinkBinsOptions { relocatable_root: Some(root.clone()), ..LinkBinsOptions::default() };
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(node_dir, Arc::new(manifest))],
        &bin_dir,
        &options,
    )
    .unwrap();

    assert_eq!(fs::read_link(bin_dir.join("node")).unwrap(), Path::new("../node/bin/node"));
    let moved = tmp.path().join("moved");
    fs::rename(&root, &moved).unwrap();
    assert_eq!(read_to_string(moved.join("node_modules/.bin/node")).unwrap(), "fake-node-binary");
}
