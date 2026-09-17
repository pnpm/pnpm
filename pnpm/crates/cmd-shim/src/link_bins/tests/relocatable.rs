use super::{
    Arc, Host, LinkBinsOptions, PackageBinSource, Path, PathBuf, create_dir_all, json,
    link_bins_of_packages, read_to_string, tempdir, write_file,
};
use crate::link_bins::bin_dir_is_relocatable;
use pnpm_fs::lexical_normalize;
use std::{
    fs,
    os::unix::fs::{MetadataExt, symlink},
    process::Command,
};

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

    let mut through_absolute_dir_symlink = Command::new(base.join("bin-link/foo"));
    through_absolute_dir_symlink.current_dir(&base);
    let invocations = [
        ("absolute through directory symlink", through_absolute_dir_symlink),
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

#[test]
fn warm_install_upgrades_absolute_node_symlink_before_relocation() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("project");
    let node_dir = root.join("node_modules/node");
    fs::create_dir_all(node_dir.join("bin")).unwrap();
    fs::write(node_dir.join("bin/node"), "fake-node-binary").unwrap();
    let packages = [PackageBinSource::new(
        node_dir,
        Arc::new(json!({
            "name": "node", "version": "20.0.0", "bin": {"node": "bin/node"}
        })),
    )];
    let bin_dir = root.join("node_modules/.bin");
    link_bins_of_packages::<Host>(&packages, &bin_dir, &LinkBinsOptions::default()).unwrap();
    assert!(fs::read_link(bin_dir.join("node")).unwrap().is_absolute());
    link_bins_of_packages::<Host>(
        &packages,
        &bin_dir,
        &LinkBinsOptions { relocatable_root: Some(root.clone()), ..LinkBinsOptions::default() },
    )
    .unwrap();
    assert_eq!(fs::read_link(bin_dir.join("node")).unwrap(), Path::new("../node/bin/node"));
    let original = fs::symlink_metadata(bin_dir.join("node")).unwrap();
    link_bins_of_packages::<Host>(
        &packages,
        &bin_dir,
        &LinkBinsOptions { relocatable_root: Some(root.clone()), ..LinkBinsOptions::default() },
    )
    .unwrap();
    assert_eq!(fs::symlink_metadata(bin_dir.join("node")).unwrap().ino(), original.ino());
    let moved = tmp.path().join("moved");
    fs::rename(root, &moved).unwrap();
    assert_eq!(
        fs::read_to_string(moved.join("node_modules/.bin/node")).unwrap(),
        "fake-node-binary",
    );
}

#[test]
fn node_bin_link_uses_the_physical_bin_directory() {
    for terminal in [false, true] {
        let tmp = tempdir().unwrap();
        let root = dunce::canonicalize(tmp.path()).unwrap().join("project");
        create_dir_all(root.join("deep/physical/.bin")).unwrap();
        create_dir_all(root.join("node-package")).unwrap();
        let runtime = tmp.path().join("external-node");
        write_file(&runtime, "fake-node-binary").unwrap();
        symlink(&runtime, root.join("node-package/node")).unwrap();
        let (alias_target, bin_dir) = if terminal {
            ("deep/physical/.bin", root.join("alias"))
        } else {
            ("deep/physical", root.join("alias/.bin"))
        };
        symlink(alias_target, root.join("alias")).unwrap();
        let packages = [PackageBinSource::new(
            root.join("node-package"),
            Arc::new(json!({"name": "node", "version": "20.0.0", "bin": "node"})),
        )];
        link_bins_of_packages::<Host>(
            &packages,
            &bin_dir,
            &LinkBinsOptions { relocatable_root: Some(root.clone()), ..LinkBinsOptions::default() },
        )
        .unwrap();
        assert_eq!(
            fs::read_link(bin_dir.join("node")).unwrap(),
            Path::new("../../../node-package/node"),
        );
        assert_eq!(read_to_string(bin_dir.join("node")).unwrap(), "fake-node-binary");
        let relative_bin = bin_dir.strip_prefix(&root).unwrap();
        let moved = tmp.path().join("moved");
        fs::rename(&root, &moved).unwrap();
        assert_eq!(
            read_to_string(moved.join(relative_bin).join("node")).unwrap(),
            "fake-node-binary",
        );
    }
}

#[test]
fn extra_node_path_through_directory_symlinks_resolves_before_and_after_move() {
    for terminal in [false, true] {
        let tmp = tempdir().unwrap();
        let root = dunce::canonicalize(tmp.path()).unwrap().join("project");
        create_dir_all(root.join("deep/physical/.bin")).unwrap();
        create_dir_all(root.join("package")).unwrap();
        symlink("deep/physical", root.join("alias")).unwrap();
        create_dir_all(root.join("deep/extras")).unwrap();
        symlink("deep/extras", root.join("extra-alias")).unwrap();
        let bin_dir = if terminal {
            symlink("deep/physical/.bin", root.join("bin-alias")).unwrap();
            root.join("bin-alias")
        } else {
            root.join("alias/.bin")
        };
        write_file(
            root.join("package/cli.js"),
            "#!/usr/bin/env node\nconsole.log(require('only-extra'))\n",
        )
        .unwrap();
        link_bins_of_packages::<Host>(
            &[PackageBinSource::new(
                root.join("package"),
                Arc::new(json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"})),
            )],
            &bin_dir,
            &LinkBinsOptions {
                extra_node_paths: vec![
                    root.join("extra-alias/missing/modules")
                        .to_string_lossy()
                        .into_owned(),
                ],
                relocatable_root: Some(root.clone()),
                ..LinkBinsOptions::default()
            },
        )
        .unwrap();
        // The extra path may be materialized after the bins are linked.
        create_dir_all(root.join("deep/extras/missing/modules/only-extra")).unwrap();
        write_file(
            root.join("deep/extras/missing/modules/only-extra/index.js"),
            "module.exports = 'resolved-extra'\n",
        )
        .unwrap();
        let relative_bin = bin_dir
            .strip_prefix(&root)
            .unwrap()
            .to_path_buf();
        let moved = tmp.path().join("moved");
        for current in [&root, &moved] {
            if current == &moved {
                fs::rename(&root, &moved).unwrap();
            }
            let output = Command::new(current.join(&relative_bin).join("foo"))
                .env_remove("NODE_PATH")
                .env_remove("NODE_OPTIONS")
                .output()
                .unwrap();
            eprintln!("terminal={terminal}, root={current:?}, output={output:?}");
            assert!(output.status.success());
            assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "resolved-extra");
        }
    }
}

#[test]
fn physical_paths_use_the_resolved_relocation_root() {
    let tmp = tempdir().unwrap();
    let physical_root = dunce::canonicalize(tmp.path()).unwrap().join("project");
    create_dir_all(&physical_root).unwrap();
    let alias = tmp.path().join("project-alias");
    symlink(&physical_root, &alias).unwrap();
    let bin_dir = link_node_path_printer(&alias, Some(alias.clone()));
    let body = read_to_string(bin_dir.join("foo")).unwrap();
    eprintln!("{body}");
    assert!(!body.contains(physical_root.to_str().unwrap()));
    assert!(!body.contains(alias.to_str().unwrap()));
    let moved = tmp.path().join("moved");
    fs::rename(&physical_root, &moved).unwrap();
    let output = Command::new(moved.join("node_modules/.bin/foo"))
        .env_remove("NODE_PATH")
        .output()
        .unwrap();
    eprintln!("{output:?}");
    assert!(output.status.success());
    for entry in String::from_utf8(output.stdout).unwrap().split(':') {
        assert!(Path::new(entry).is_dir(), "NODE_PATH entry does not exist: {entry}");
    }
}

#[test]
fn bins_outside_the_physical_root_keep_the_global_shim_body() {
    let tmp = tempdir().unwrap();
    let base = dunce::canonicalize(tmp.path()).unwrap();
    let root = base.join("project");
    let outside = base.join("outside");
    create_dir_all(&root).unwrap();
    create_dir_all(&outside).unwrap();
    symlink("../outside", root.join("alias")).unwrap();
    let bin_dir = link_node_path_printer(&root.join("alias"), None);
    let original = read_to_string(bin_dir.join("foo")).unwrap();
    fs::remove_file(bin_dir.join("foo")).unwrap();
    link_node_path_printer(&root.join("alias"), Some(root));
    assert_eq!(read_to_string(bin_dir.join("foo")).unwrap(), original);
}

#[test]
fn external_node_path_alias_keeps_its_original_spelling() {
    let tmp = tempdir().unwrap();
    let base = dunce::canonicalize(tmp.path()).unwrap();
    let root = base.join("project");
    create_dir_all(root.join("package")).unwrap();
    create_dir_all(base.join("external")).unwrap();
    symlink("external", base.join("external-alias")).unwrap();
    let extra = base
        .join("external-alias/modules")
        .to_string_lossy()
        .into_owned();
    write_file(root.join("package/cli"), "#!/bin/sh\nprintf '%s' \"$NODE_PATH\"\n").unwrap();
    let bins = root.join(".bin");
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(
            root.join("package"),
            Arc::new(json!({"name": "foo", "version": "1.0.0", "bin": "cli"})),
        )],
        &bins,
        &LinkBinsOptions {
            extra_node_paths: vec![extra.clone()],
            relocatable_root: Some(root),
            ..LinkBinsOptions::default()
        },
    )
    .unwrap();
    let output = Command::new(bins.join("foo"))
        .env_remove("NODE_PATH")
        .output()
        .unwrap();
    eprintln!("{output:?}");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap()
            .split(':')
            .next_back(),
        Some(extra.as_str()),
    );
}

#[test]
fn physical_target_without_node_path_runs_through_a_directory_symlink() {
    let tmp = tempdir().unwrap();
    let root = dunce::canonicalize(tmp.path()).unwrap().join("project");
    create_dir_all(root.join("deep/physical/.bin")).unwrap();
    create_dir_all(root.join("package")).unwrap();
    symlink("deep/physical", root.join("alias")).unwrap();
    write_file(
        root.join("package/cli.js"),
        "#!/usr/bin/env node\nconsole.log('physical-target')\n",
    )
    .unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(
            root.join("package"),
            Arc::new(json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"})),
        )],
        &root.join("alias/.bin"),
        &LinkBinsOptions { relocatable_root: Some(root.clone()), ..LinkBinsOptions::default() },
    )
    .unwrap();
    let moved = tmp.path().join("moved");
    fs::rename(&root, &moved).unwrap();
    let output = Command::new(moved.join("alias/.bin/foo"))
        .env_remove("NODE_PATH")
        .env_remove("NODE_OPTIONS")
        .output()
        .unwrap();
    eprintln!("{output:?}");
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "physical-target");
}

#[test]
fn extra_node_path_escaping_root_stays_absolute() {
    let tmp = tempdir().unwrap();
    let base = dunce::canonicalize(tmp.path()).unwrap();
    let root = base.join("project");
    create_dir_all(root.join("package")).unwrap();
    let external = base.join("external");
    create_dir_all(&external).unwrap();
    symlink(&external, root.join("external-alias")).unwrap();
    let root_alias = base.join("project-alias");
    symlink(&root, &root_alias).unwrap();
    write_file(root.join("package/cli"), "#!/bin/sh\nprintf '%s' \"$NODE_PATH\"\n").unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(
            root.join("package"),
            Arc::new(json!({"name": "foo", "version": "1.0.0", "bin": "cli"})),
        )],
        &root.join(".bin"),
        &LinkBinsOptions {
            extra_node_paths: vec![
                root_alias
                    .join("external-alias/modules")
                    .to_string_lossy()
                    .into_owned(),
            ],
            relocatable_root: Some(root_alias),
            ..LinkBinsOptions::default()
        },
    )
    .unwrap();
    let moved = base.join("moved");
    fs::rename(&root, &moved).unwrap();
    let output = Command::new(moved.join(".bin/foo"))
        .env_remove("NODE_PATH")
        .output()
        .unwrap();
    eprintln!("{output:?}");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .unwrap()
            .split(':')
            .next_back(),
        external.join("modules").to_str(),
    );
}

#[test]
fn warm_install_upgrades_physical_target_anchor_without_node_path() {
    let tmp = tempdir().unwrap();
    let root = dunce::canonicalize(tmp.path()).unwrap().join("project");
    create_dir_all(root.join("package")).unwrap();
    write_file(root.join("package/cli.js"), "#!/usr/bin/env node\nconsole.log('upgraded')\n")
        .unwrap();
    let packages = [PackageBinSource::new(
        root.join("package"),
        Arc::new(json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"})),
    )];
    let bins = root.join(".bin");
    let options = LinkBinsOptions { relocatable_root: Some(root), ..LinkBinsOptions::default() };
    link_bins_of_packages::<Host>(&packages, &bins, &options).unwrap();
    let shim = bins.join("foo");
    let current = read_to_string(&shim).unwrap();
    let old = current.replace("$basedir_abs/", "$basedir/");
    write_file(&shim, &old).unwrap();
    link_bins_of_packages::<Host>(&packages, &bins, &options).unwrap();
    assert_eq!(read_to_string(&shim).unwrap(), current);
    let inode = fs::symlink_metadata(&shim).unwrap().ino();
    link_bins_of_packages::<Host>(&packages, &bins, &options).unwrap();
    assert_eq!(fs::symlink_metadata(&shim).unwrap().ino(), inode);
}

#[test]
fn external_target_runs_through_a_physical_bin_directory() {
    let tmp = tempdir().unwrap();
    let base = dunce::canonicalize(tmp.path()).unwrap();
    let root = base.join("project");
    create_dir_all(root.join("deep/physical/.bin")).unwrap();
    create_dir_all(base.join("external-package")).unwrap();
    symlink("deep/physical", root.join("alias")).unwrap();
    write_file(
        base.join("external-package/cli.js"),
        "#!/usr/bin/env node\nconsole.log('external-target')\n",
    )
    .unwrap();
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(
            base.join("external-package"),
            Arc::new(json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"})),
        )],
        &root.join("alias/.bin"),
        &LinkBinsOptions { relocatable_root: Some(root.clone()), ..LinkBinsOptions::default() },
    )
    .unwrap();
    let shim = root.join("alias/.bin/foo");
    let body = read_to_string(&shim).unwrap();
    eprintln!("{body}");
    assert!(body.ends_with(&format!(
        "# cmd-shim-target={}\n",
        base.join("external-package/cli.js").display()
    )));
    let output = Command::new(&shim)
        .env_remove("NODE_PATH")
        .env_remove("NODE_OPTIONS")
        .output()
        .unwrap();
    eprintln!("{output:?}");
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "external-target");
}

#[test]
fn unresolved_optional_node_paths_do_not_prevent_linking_bins() {
    for destination in ["missing-modules", "extra"] {
        let tmp = tempdir().unwrap();
        let root = dunce::canonicalize(tmp.path()).unwrap().join("project");
        create_dir_all(root.join("package")).unwrap();
        let extra = root.join("extra");
        symlink(destination, &extra).unwrap();
        write_file(
            root.join("package/cli.js"),
            "#!/usr/bin/env node\nconsole.log(process.argv[2] === 'require' ? require('late') : 'linked')\n",
        ).unwrap();
        let bins = root.join(".bin");
        link_bins_of_packages::<Host>(
            &[PackageBinSource::new(
                root.join("package"),
                Arc::new(json!({
                    "name": "tools", "version": "1.0.0", "bin": {"foo": "cli.js", "bar": "cli.js"}
                })),
            )],
            &bins,
            &LinkBinsOptions {
                extra_node_paths: vec![extra.to_string_lossy().into_owned()],
                relocatable_root: Some(root.clone()),
                ..LinkBinsOptions::default()
            },
        )
        .unwrap();
        for name in ["foo", "bar"] {
            let body = read_to_string(bins.join(name)).unwrap();
            eprintln!("destination={destination}, body={body}");
            assert!(body.contains("$basedir_abs/../extra"), "optional path remains in NODE_PATH");
            let output = Command::new(bins.join(name))
                .env_remove("NODE_PATH")
                .env_remove("NODE_OPTIONS")
                .output()
                .unwrap();
            eprintln!("destination={destination}, name={name}, output={output:?}");
            assert!(output.status.success());
            assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "linked");
        }
        fs::remove_file(&extra).unwrap();
        symlink("missing-modules", &extra).unwrap();
        create_dir_all(root.join("missing-modules/late")).unwrap();
        write_file(root.join("missing-modules/late/index.js"), "module.exports = 'loaded-later'\n")
            .unwrap();
        let output = Command::new(bins.join("foo"))
            .arg("require")
            .env_remove("NODE_PATH")
            .env_remove("NODE_OPTIONS")
            .output()
            .unwrap();
        eprintln!("destination={destination}, repaired output={output:?}");
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "loaded-later");
    }
}

#[test]
fn external_target_directory_alias_can_be_retargeted_after_linking() {
    let tmp = tempdir().unwrap();
    let base = dunce::canonicalize(tmp.path()).unwrap();
    let root = base.join("project");
    for name in ["first", "second"] {
        create_dir_all(base.join(name)).unwrap();
        write_file(
            base.join(name).join("cli.js"),
            format!("#!/usr/bin/env node\nconsole.log('{name}')\n"),
        )
        .unwrap();
    }
    let alias = base.join("external-alias");
    symlink("first", &alias).unwrap();
    let bins = root.join(".bin");
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(
            alias.clone(),
            Arc::new(json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"})),
        )],
        &bins,
        &LinkBinsOptions { relocatable_root: Some(root), ..LinkBinsOptions::default() },
    )
    .unwrap();
    fs::remove_file(&alias).unwrap();
    symlink("second", &alias).unwrap();
    let output = Command::new(bins.join("foo"))
        .env_remove("NODE_PATH")
        .env_remove("NODE_OPTIONS")
        .output()
        .unwrap();
    eprintln!("{output:?}");
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "second");
}

#[test]
fn bin_dir_is_relocatable_accepts_generated_and_rejects_absolute_symlinks() {
    let tmp = tempdir().unwrap();
    let root = dunce::canonicalize(tmp.path()).unwrap();
    let bin_dir = link_node_path_printer(&root, Some(root.clone()));
    symlink("../.pnpm/foo@1.0.0/node_modules/foo/print.sh", bin_dir.join("linked")).unwrap();
    assert!(bin_dir_is_relocatable(&bin_dir, &root));
    assert!(bin_dir_is_relocatable(&root.join("missing/.bin"), &root), "nothing to move");
    assert!(!bin_dir_is_relocatable(&bin_dir.join("foo"), &root), "unreadable, so it fails closed");

    let legacy_root = root.join("legacy");
    let legacy_bin_dir = link_node_path_printer(&legacy_root, None);
    assert!(!bin_dir_is_relocatable(&legacy_bin_dir, &legacy_root), "absolute shims");

    let outside_links = [
        root.join("node_modules/.pnpm/foo@1.0.0/node_modules/foo/print.sh"),
        PathBuf::from("../../../outside"),
    ];
    for outside_link in outside_links {
        let link = bin_dir.join("outside");
        symlink(&outside_link, &link).unwrap();
        assert!(!bin_dir_is_relocatable(&bin_dir, &root), "{outside_link:?}");
        fs::remove_file(&link).unwrap();
    }

}
