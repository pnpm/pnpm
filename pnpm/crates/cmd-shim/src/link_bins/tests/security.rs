use super::{
    Arc, Host, LinkBinsOptions, PackageBinSource, Path, Value, create_dir_all, json,
    link_bins_of_packages, read_file, read_to_string, tempdir, write_file,
};
#[cfg(unix)]
use super::{PathBuf, is_sh_shim_hardened, is_shim_pointing_at, remove_bin};
#[cfg(unix)]
use crate::shim::generate_sh_shim;
#[cfg(unix)]
use std::{fs::metadata, os::unix::fs::symlink};

#[cfg(unix)]
fn javascript_package(root: &Path) -> (PathBuf, Value) {
    let package = root.join("node_modules/foo");
    create_dir_all(&package).unwrap();
    let target = package.join("cli.js");
    write_file(
        &target,
        "#!/usr/bin/env node\nconsole.log(JSON.stringify({argv: process.argv[1], filename: __filename}))\n",
    )
    .unwrap();
    let manifest = json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"});
    (package, serde_json::from_value(manifest).unwrap())
}

#[cfg(unix)]
fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .unwrap()
        .file_type()
        .is_symlink()
}

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
    let cli_js = pkg_dir.join("cli.js");
    write_file(
        pkg_dir.join("package.json"),
        json!({"name": "foo", "version": "1.0.0", "bin": "cli.js"}).to_string(),
    )
    .unwrap();
    write_file(&cli_js, "#!/usr/bin/env node\nconsole.log('hello_world')\n").unwrap();

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

    let bin = bins_dir.join("foo");
    if cfg!(windows) {
        // The setting is inert on Windows: bins keep their shims.
        assert!(
            std::fs::symlink_metadata(&bin)
                .unwrap()
                .file_type()
                .is_file(),
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
            metadata(&cli_js)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755,
            "the target file gets the executable bits, like pnpm's ensureExecutable",
        );
    }
}

#[cfg(unix)]
#[test]
fn preserve_bin_name_keeps_a_shim_and_runs_the_alias() {
    let tmp = tempdir().unwrap();
    let (package, manifest_value) = javascript_package(tmp.path());
    let target = package.join("cli.js");
    let bins_dir = tmp.path().join("node_modules/.bin");
    let options = LinkBinsOptions {
        extra_node_paths: vec!["/tmp/hoisted".into()],
        prefer_symlinked_executables: true,
        preserve_bin_name: true,
        relocatable_root: Some(tmp.path().to_path_buf()),
        ..LinkBinsOptions::default()
    };
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(package, Arc::new(manifest_value))],
        &bins_dir,
        &options,
    )
    .unwrap();

    let shim = bins_dir.join("foo");
    let alias = bins_dir
        .parent()
        .unwrap()
        .join(".bin-symlinks/foo");
    assert!(!is_symlink(&shim));
    assert!(is_symlink(&alias));
    assert_eq!(std::fs::read_link(&alias).unwrap(), Path::new("../foo/cli.js"));
    let shim_body = read_to_string(&shim).unwrap();
    assert!(shim_body.contains("../.bin-symlinks/foo"));
    assert!(shim_body.contains("export NODE_PATH="));

    let output = std::process::Command::new(&shim).output().unwrap();
    assert!(output.status.success(), "bin failed: {output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(alias.to_string_lossy().as_ref()), "argv was: {stdout}");
    assert!(stdout.contains(target.to_string_lossy().as_ref()), "filename was: {stdout}");

    super::remove_bin(&shim).unwrap();
    assert!(!alias.exists());
    assert!(alias.parent().unwrap().exists());
}

#[cfg(unix)]
#[test]
fn failed_default_relink_leaves_the_existing_alias_untouched() {
    let tmp = tempdir().unwrap();
    let (package, manifest) = javascript_package(tmp.path());
    let bins_dir = tmp.path().join("node_modules/.bin");
    let options = LinkBinsOptions { preserve_bin_name: true, ..LinkBinsOptions::default() };
    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(package.clone(), Arc::new(manifest.clone()))],
        &bins_dir,
        &options,
    )
    .unwrap();
    let shim = bins_dir.join("foo");
    let alias = bins_dir.with_file_name(".bin-symlinks").join("foo");
    let old_target = std::fs::read_link(&alias).unwrap();
    std::fs::remove_file(&shim).unwrap();
    std::fs::create_dir(&shim).unwrap();

    let error = link_bins_of_packages::<Host>(
        &[PackageBinSource::new(package, Arc::new(manifest))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .expect_err("the directory squatting the shim path must fail the relink");

    assert!(matches!(error, super::LinkBinsError::WriteShim { .. }));
    assert_eq!(std::fs::read_link(&alias).unwrap(), old_target);
}

#[cfg(unix)]
#[test]
fn removing_one_alias_keeps_the_alias_directory_when_other_aliases_exist() {
    let tmp = tempdir().unwrap();
    let bins_dir = tmp.path().join("node_modules/.bin");
    let alias_dir = tmp.path().join("node_modules/.bin-symlinks");
    create_dir_all(&bins_dir).unwrap();
    create_dir_all(&alias_dir).unwrap();
    let target = tmp.path().join("target.js");
    write_file(&target, "target").unwrap();
    symlink(&target, alias_dir.join("foo")).unwrap();
    symlink(&target, alias_dir.join("bar")).unwrap();
    write_file(bins_dir.join("foo"), "shim").unwrap();

    remove_bin(&bins_dir.join("foo")).unwrap();

    assert!(!alias_dir.join("foo").exists());
    assert!(alias_dir.join("bar").exists());
    assert!(alias_dir.exists());
}

#[cfg(unix)]
#[test]
fn preserve_bin_name_rejects_a_symlinked_alias_directory() {
    let tmp = tempdir().unwrap();
    let (package, manifest) = javascript_package(tmp.path());
    let victim = tmp.path().join("victim");
    create_dir_all(&victim).unwrap();
    write_file(victim.join("foo"), "keep").unwrap();
    let alias_dir = tmp.path().join("node_modules/.bin-symlinks");
    symlink(&victim, &alias_dir).unwrap();

    let error = link_bins_of_packages::<Host>(
        &[PackageBinSource::new(package, Arc::new(manifest))],
        &tmp.path().join("node_modules/.bin"),
        &LinkBinsOptions { preserve_bin_name: true, ..LinkBinsOptions::default() },
    )
    .unwrap_err();

    assert!(matches!(error, super::LinkBinsError::CreateAliasDir { .. }));
    assert_eq!(read_to_string(victim.join("foo")).unwrap(), "keep");
}

#[cfg(unix)]
#[test]
fn default_bin_linking_does_not_touch_a_symlinked_alias_directory() {
    let tmp = tempdir().unwrap();
    let (package, manifest) = javascript_package(tmp.path());
    let victim = tmp.path().join("victim");
    create_dir_all(&victim).unwrap();
    write_file(victim.join("foo"), "keep").unwrap();
    let alias_dir = tmp.path().join("node_modules/.bin-symlinks");
    symlink(&victim, &alias_dir).unwrap();

    let error = link_bins_of_packages::<Host>(
        &[PackageBinSource::new(package, Arc::new(manifest))],
        &tmp.path().join("node_modules/.bin"),
        &LinkBinsOptions::default(),
    )
    .unwrap_err();

    assert!(matches!(error, super::LinkBinsError::RemoveStaleBin { .. }));
    assert_eq!(read_to_string(victim.join("foo")).unwrap(), "keep");
}

#[cfg(unix)]
#[test]
fn removing_a_bin_does_not_follow_a_symlinked_alias_directory() {
    let tmp = tempdir().unwrap();
    let bins_dir = tmp.path().join("node_modules/.bin");
    create_dir_all(&bins_dir).unwrap();
    let shim = bins_dir.join("foo");
    write_file(&shim, "shim").unwrap();
    let victim = tmp.path().join("victim");
    create_dir_all(&victim).unwrap();
    let victim_bin = victim.join("foo");
    write_file(&victim_bin, "keep").unwrap();
    symlink(&victim, bins_dir.with_file_name(".bin-symlinks")).unwrap();

    assert!(remove_bin(&shim).is_err());
    assert_eq!(read_to_string(victim_bin).unwrap(), "keep");
}

#[cfg(unix)]
#[test]
fn preserve_bin_name_replaces_an_existing_direct_symlink() {
    let tmp = tempdir().unwrap();
    let (package, manifest) = javascript_package(tmp.path());
    let bins_dir = tmp.path().join("node_modules/.bin");
    let package_source = PackageBinSource::new(package, Arc::new(manifest));
    link_bins_of_packages::<Host>(
        std::slice::from_ref(&package_source),
        &bins_dir,
        &LinkBinsOptions { prefer_symlinked_executables: true, ..LinkBinsOptions::default() },
    )
    .unwrap();
    let shim = bins_dir.join("foo");
    assert!(is_symlink(&shim));

    link_bins_of_packages::<Host>(
        &[package_source],
        &bins_dir,
        &LinkBinsOptions {
            prefer_symlinked_executables: true,
            preserve_bin_name: true,
            ..LinkBinsOptions::default()
        },
    )
    .unwrap();

    let alias = bins_dir.with_file_name(".bin-symlinks").join("foo");
    assert!(!is_symlink(&shim));
    assert!(is_symlink(&alias));
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
    assert!(is_shim_pointing_at(&body, &bins_dir.join("foo"), &pkg.join("cli.js")));
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
        !std::fs::symlink_metadata(bins_dir.join("foo"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the shim is a regular file",
    );
    let body = read_to_string(bins_dir.join("foo")).unwrap();
    assert!(is_shim_pointing_at(&body, &bins_dir.join("foo"), &pkg.join("cli.js")));
}

/// A shim an older pacquet wrote still points at the right target, so the warm
/// reinstall path had nothing to notice and left it in place. It resolved
/// `readlink` and its other helpers on the caller's `PATH`, which starts with
/// the very directory the shim lives in, so upgrading pnpm has to replace it.
#[cfg(unix)]
#[test]
fn a_reinstall_replaces_a_shim_that_looks_its_helpers_up_on_the_callers_path() {
    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let target = pkg.join("cli.js");
    // Pre-created, so the reinstall reads what is there instead of taking the
    // fresh-write path a newly made bin directory gets.
    let bins_dir = tmp.path().join(".bin");
    create_dir_all(&bins_dir).unwrap();
    let shim = bins_dir.join("foo");
    let outdated = format!(
        r#"#!/bin/sh
link="$0"
hops=0
while [ -L "$link" ] && [ "$hops" -lt 40 ]; do
  hops=$((hops+1))
  target=$(readlink "$link")
  case "$target" in
    /*) link="$target" ;;
    *)  link="$(dirname "$link")/$target" ;;
  esac
done
basedir=$(dirname "$(echo "$link" | sed -e 's,\\,/,g')")
exec node  "$basedir/../foo/cli.js" "$@"
# cmd-shim-target={}
"#,
        target.display(),
    );
    write_file(&shim, &outdated).unwrap();
    assert!(
        is_shim_pointing_at(&outdated, &shim, &target),
        "precondition: the outdated shim carries a matching target marker, so only the \
         header tells it apart from a current one",
    );
    assert!(!is_sh_shim_hardened(&outdated), "precondition: the outdated shim is not hardened");

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(&shim).unwrap();
    assert!(is_shim_pointing_at(&body, &shim, &target), "the rewritten shim keeps its target");
    assert!(
        is_sh_shim_hardened(&body),
        "the reinstall must replace a shim that resolves its helpers on PATH, body was:\n{body}",
    );
}

/// A shim that already looks up `readlink` with `command -p` can still pipe
/// `$link` through `echo`. The target marker matches, so a warm reinstall
/// has to notice the conversion line and replace the shim.
#[cfg(unix)]
#[test]
fn a_reinstall_replaces_a_shim_that_pipes_the_path_through_echo() {
    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let target = pkg.join("cli.js");
    let bins_dir = tmp.path().join(".bin");
    create_dir_all(&bins_dir).unwrap();
    let shim = bins_dir.join("foo");
    let outdated = format!(
        r#"#!/bin/sh
link="$0"
case "$link" in
  */*|*\\*) ;;
  *) link="./$link" ;;
esac
hops=0
while [ -L "$link" ] && [ "$hops" -lt 40 ]; do
  hops=$((hops+1))
  target=$(command -p readlink "$link")
  case "$target" in
    /*) link="$target" ;;
    *)  link="${{link%/*}}/$target" ;;
  esac
done
basedir=$(echo "$link" | command -p sed -e 's,\\,/,g')
basedir="${{basedir%/*}}"
exec node  "$basedir/../foo/cli.js" "$@"
# cmd-shim-target={}
"#,
        target.display(),
    );
    write_file(&shim, &outdated).unwrap();
    assert!(
        is_shim_pointing_at(&outdated, &shim, &target),
        "precondition: the outdated shim carries a matching target marker",
    );
    assert!(
        outdated.contains(r#"  target=$(command -p readlink "$link")"#),
        "precondition: helpers already resolve off the system path",
    );
    assert!(!is_sh_shim_hardened(&outdated), "precondition: the echo conversion is not current");

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(&shim).unwrap();
    assert!(is_shim_pointing_at(&body, &shim, &target), "the rewritten shim keeps its target");
    assert!(
        is_sh_shim_hardened(&body),
        "the reinstall must replace a shim that pipes $link through echo, body was:\n{body}",
    );
}

/// A shim can carry the hardened `readlink` and `printf` lines and still take
/// its Windows path conversion from the caller's `PATH`. The target marker
/// matches, so a warm reinstall has to notice the conversion helpers and
/// replace the shim.
#[cfg(unix)]
#[test]
fn a_reinstall_replaces_a_shim_that_converts_paths_with_a_helper_from_the_callers_path() {
    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let target = pkg.join("cli.js");
    let bins_dir = tmp.path().join(".bin");
    create_dir_all(&bins_dir).unwrap();
    let shim = bins_dir.join("foo");
    let outdated = generate_sh_shim(&target, &shim, None, &[], None)
        .replace("command -p cygpath", "cygpath")
        .replace("command -p wslpath", "wslpath");
    write_file(&shim, &outdated).unwrap();
    assert!(
        is_shim_pointing_at(&outdated, &shim, &target),
        "precondition: the outdated shim carries a matching target marker",
    );
    assert!(
        outdated.contains(r#"  target=$(command -p readlink "$link")"#),
        "precondition: the other helpers already resolve off the system path",
    );
    assert!(
        !is_sh_shim_hardened(&outdated),
        "precondition: PATH-resolved path conversion is not current",
    );

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(&shim).unwrap();
    assert!(is_shim_pointing_at(&body, &shim, &target), "the rewritten shim keeps its target");
    assert!(
        is_sh_shim_hardened(&body),
        "the reinstall must replace a shim that converts paths with a helper from PATH, \
         body was:\n{body}",
    );
}

/// A shim can resolve every helper through `command -p` and still leave the
/// caller's `node_modules` entries on the `PATH` that `command -p` searches on
/// Nix. The target marker matches, so a warm reinstall has to notice the missing
/// filter and replace the shim.
#[cfg(unix)]
#[test]
fn a_reinstall_replaces_a_shim_that_resolves_helpers_with_node_modules_on_path() {
    let manifest = serde_json::json!({"name": "foo", "bin": "cli.js"});
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("foo");
    create_dir_all(&pkg).unwrap();
    write_file(pkg.join("cli.js"), "#!/usr/bin/env node\n").unwrap();
    let target = pkg.join("cli.js");
    let bins_dir = tmp.path().join(".bin");
    create_dir_all(&bins_dir).unwrap();
    let shim = bins_dir.join("foo");
    let outdated = generate_sh_shim(&target, &shim, None, &[], None)
        .replace("    */node_modules/*|*/node_modules) ;;\n", "");
    write_file(&shim, &outdated).unwrap();
    assert!(
        is_shim_pointing_at(&outdated, &shim, &target),
        "precondition: the outdated shim carries a matching target marker",
    );
    assert!(!is_sh_shim_hardened(&outdated), "precondition: the unfiltered PATH is not current");

    link_bins_of_packages::<Host>(
        &[PackageBinSource::new(pkg, Arc::new(manifest))],
        &bins_dir,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(&shim).unwrap();
    assert!(is_shim_pointing_at(&body, &shim, &target), "the rewritten shim keeps its target");
    assert!(
        is_sh_shim_hardened(&body),
        "the reinstall must replace a shim that keeps node_modules on the helpers' PATH, \
         body was:\n{body}",
    );
}
