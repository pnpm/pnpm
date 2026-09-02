use super::{
    FsGlobalRemoval, GlobalInstallCleanup, GlobalRemovalTransaction, activation::FsRename,
    check_virtual_shim_conflicts, commit_global_removal, infer_local_package_alias,
    is_windows_drive_path, replacement_aliases, resolve_local_param,
    should_replace_existing_package, split_comma_separated, update_selectors,
};
use miette::IntoDiagnostic;
use pnpm_cmd_shim::{Host as CmdShimHost, PackageBinSource, remove_bin as remove_cmd_shim};
use pnpm_fs::{force_symlink_dir, remove_symlink_dir};
use pnpm_global::GlobalPackageInfo;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::TempDir;

use crate::{
    cli_args::shim::record_virtual_shim_state,
    shim_dispatch::{ShimTarget, install_native_shim, remove_native_shim},
};

struct BinRemovalFailure;
struct HashRemovalFailure;

impl FsRename for BinRemovalFailure {
    fn rename(source: &Path, target: &Path) -> io::Result<()> {
        <CmdShimHost as FsRename>::rename(source, target)
    }
}

impl FsGlobalRemoval for BinRemovalFailure {
    fn remove_bin_slot(path: &Path) -> io::Result<()> {
        if path.ends_with("other") {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected other-bin removal failure",
            ));
        }
        remove_cmd_shim(path)
    }
}

impl FsRename for HashRemovalFailure {
    fn rename(source: &Path, target: &Path) -> io::Result<()> {
        <CmdShimHost as FsRename>::rename(source, target)
    }
}

impl FsGlobalRemoval for HashRemovalFailure {
    fn remove_hash_link(path: &Path) -> io::Result<()> {
        if path.ends_with("group-hash") {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected hash-link removal failure",
            ));
        }
        remove_symlink_dir(path)
    }
}

#[test]
fn comma_splits_into_selectors() {
    let base = Path::new("/nonexistent");
    assert_eq!(split_comma_separated("foo,bar", base), vec!["foo", "bar"]);
    assert_eq!(split_comma_separated("foo", base), vec!["foo"]);
}

#[test]
fn urls_are_kept_whole() {
    let base = Path::new("/nonexistent");
    assert_eq!(
        split_comma_separated("https://example.com/a,b.tgz", base),
        vec!["https://example.com/a,b.tgz"],
    );
}

#[test]
fn a_virtual_shim_only_yields_to_its_own_package() {
    let root = tempfile::tempdir().expect("create temp directory");
    let bin_dir = root.path().join("bin");
    let package_dir = root.path().join("package");
    fs::create_dir_all(&package_dir).expect("create package directory");
    fs::write(package_dir.join("cli.js"), "").expect("write package bin");
    install_native_shim(&bin_dir, "tool", &ShimTarget::Virtual("owner".to_string()))
        .expect("link virtual shim");

    let owner = PackageBinSource::new(
        package_dir.clone(),
        Arc::new(json!({ "name": "owner", "bin": { "tool": "cli.js" } })),
    );
    check_virtual_shim_conflicts(&[owner], &bin_dir).expect("allow the owning package");

    let unrelated = PackageBinSource::new(
        package_dir.clone(),
        Arc::new(json!({ "name": "unrelated", "bin": { "tool": "cli.js" } })),
    );
    let error = check_virtual_shim_conflicts(std::slice::from_ref(&unrelated), &bin_dir)
        .unwrap_err()
        .to_string();
    assert!(error.contains(r#"project-aware shim for "owner""#), "{error}");

    remove_native_shim(&bin_dir, "tool").expect("remove virtual shim");
    fs::write(bin_dir.join("tool"), "globally installed shim").expect("replace virtual shim");
    record_virtual_shim_state(&bin_dir, "owner", &["tool".to_string()])
        .expect("record restoration state");
    let error = check_virtual_shim_conflicts(&[unrelated], &bin_dir).unwrap_err().to_string();
    assert!(error.contains(r#"project-aware shim for "owner""#), "{error}");

    let owner = PackageBinSource::new(
        package_dir,
        Arc::new(json!({ "name": "owner", "bin": { "tool": "cli.js" } })),
    );
    check_virtual_shim_conflicts(&[owner], &bin_dir).expect("allow the recorded owner");
}

#[test]
fn bin_cleanup_failure_restores_package_commands() {
    let fixture = GlobalRemovalFixture::new();
    let bins_to_keep = HashSet::from(["owner".to_string()]);
    let cleanup = fixture.cleanup(&bins_to_keep);
    let transaction = fixture.transaction(&cleanup);

    let error = commit_global_removal::<BinRemovalFailure>(&transaction, || {
        install_native_shim(
            &fixture.global_bin_dir,
            "owner",
            &ShimTarget::Virtual("owner".to_string()),
        )
        .into_diagnostic()
    })
    .expect_err("the injected bin cleanup must fail removal");

    assert!(format!("{error:?}").contains("injected other-bin removal failure"));
    fixture.assert_package_commands_restored();
}

#[test]
fn hash_cleanup_failure_restores_package_commands() {
    let fixture = GlobalRemovalFixture::new();
    let bins_to_keep = HashSet::from(["owner".to_string()]);
    let cleanup = fixture.cleanup(&bins_to_keep);
    let transaction = fixture.transaction(&cleanup);

    let error = commit_global_removal::<HashRemovalFailure>(&transaction, || {
        install_native_shim(
            &fixture.global_bin_dir,
            "owner",
            &ShimTarget::Virtual("owner".to_string()),
        )
        .into_diagnostic()
    })
    .expect_err("the injected hash cleanup must fail removal");

    assert!(format!("{error:?}").contains("injected hash-link removal failure"));
    fixture.assert_package_commands_restored();
}

#[test]
fn later_hash_cleanup_failure_restores_earlier_groups() {
    let fixture = GlobalRemovalFixture::new();
    let first_group =
        fixture.seed_group(GlobalGroupSpec { alias: "first", hash: "first-hash", bin: "first" });
    let groups = vec![first_group.clone(), fixture.group.clone()];
    let affected_bin_names =
        fixture.affected_bin_names.iter().cloned().chain(["first".to_string()]).collect();
    let bins_to_keep = HashSet::new();
    let cleanup = fixture.cleanup(&bins_to_keep);
    let transaction = GlobalRemovalTransaction {
        groups: &groups,
        cleanup: &cleanup,
        affected_bin_names: &affected_bin_names,
    };

    let error = commit_global_removal::<HashRemovalFailure>(&transaction, || Ok(()))
        .expect_err("the later injected hash cleanup must fail removal");

    assert!(format!("{error:?}").contains("injected hash-link removal failure"));
    fixture.assert_package_commands_restored();
    assert_eq!(
        fs::read(fixture.global_bin_dir.join("first")).expect("read first bin"),
        b"old first\n",
    );
    assert!(pnpm_global::get_hash_link(&fixture.global_pkg_dir, &first_group.hash).exists());
    assert!(first_group.install_dir.exists());
}

#[test]
fn detects_windows_drive_paths() {
    assert!(is_windows_drive_path(r"C:\foo"));
    assert!(is_windows_drive_path("d:/bar"));
    assert!(!is_windows_drive_path("foo"));
}

#[test]
fn latest_update_drops_the_spec_only_of_plain_version_dependencies() {
    let dependencies = vec![
        ("private-linked-pkg".to_string(), "link:/home/user/private-linked-pkg".to_string()),
        ("local-tarball-pkg".to_string(), "file:/home/user/local-tarball-pkg.tgz".to_string()),
        ("git-pkg".to_string(), "github:user/git-pkg".to_string()),
        ("remote-tarball-pkg".to_string(), "https://example.com/pkg.tgz".to_string()),
        ("aliased-pkg".to_string(), "npm:other-pkg@^2.0.0".to_string()),
        ("named-registry-pkg".to_string(), "gh:^3.0.0".to_string()),
        ("foo".to_string(), "^1.0.0".to_string()),
        ("bar".to_string(), "next".to_string()),
    ];
    assert_eq!(
        update_selectors(&dependencies, true, &HashMap::new()),
        vec![
            "private-linked-pkg@link:/home/user/private-linked-pkg",
            "local-tarball-pkg@file:/home/user/local-tarball-pkg.tgz",
            "git-pkg@github:user/git-pkg",
            "remote-tarball-pkg@https://example.com/pkg.tgz",
            "aliased-pkg@npm:other-pkg@^2.0.0",
            "named-registry-pkg@gh:^3.0.0",
            "foo",
            "bar",
        ],
    );
    assert_eq!(
        update_selectors(&dependencies, false, &HashMap::new()),
        dependencies.iter().map(|(alias, spec)| format!("{alias}@{spec}")).collect::<Vec<String>>(),
    );
}

/// An update must never move a package backwards, so a pinned alias is held at
/// the version it is already on while the rest of the group still updates.
#[test]
fn a_pinned_dependency_is_held_at_its_installed_version() {
    let dependencies = vec![
        ("prerelease".to_string(), "^2.0.0".to_string()),
        ("stable".to_string(), "^1.0.0".to_string()),
    ];
    let pins = HashMap::from([("prerelease".to_string(), "2.0.0".to_string())]);

    assert_eq!(update_selectors(&dependencies, true, &pins), vec!["prerelease@2.0.0", "stable"]);
}

#[test]
fn unnamed_local_package_uses_directory_name_as_alias() {
    let root = tempfile::tempdir().expect("create temp directory");
    let package_dir = create_local_package(root.path(), "local-package", "{}");
    let selector = format!("file:{}", package_dir.display());

    assert_eq!(
        infer_local_package_alias(&selector).expect("infer package alias"),
        format!("local-package@{selector}"),
    );
}

#[test]
fn dot_relative_file_selectors_resolve_from_the_configured_base_directory() {
    let root = tempfile::tempdir().expect("create temp directory");
    let package_dir = create_local_package(root.path(), "local-package", "{}");
    let resolved = resolve_local_param("file:.", package_dir.as_path());

    assert_eq!(
        infer_local_package_alias(&resolved).expect("infer package alias"),
        format!("local-package@{resolved}"),
    );
}

/// Parity with the TypeScript `resolveLocalParam`: non-dot `file:`/`link:`
/// selectors are left untouched. Rewriting a bare name against `base_dir`
/// would diverge from pnpm, and rewriting `file:~/…` would defeat the
/// resolver's home-directory expansion.
#[test]
fn non_dot_local_selectors_are_passed_through_unchanged() {
    let base_dir = Path::new("/base");
    for selector in ["file:local-package", "file:~/pkg", "link:~/pkg", "link:pkg"] {
        assert_eq!(resolve_local_param(selector, base_dir), selector);
    }
}

#[test]
fn parent_file_selector_uses_parent_directory_name_as_alias() {
    let root = tempfile::tempdir().expect("create temp directory");
    let package_dir = create_local_package(root.path(), "local-package", "{}");
    let child_dir = package_dir.join("child");
    std::fs::create_dir(&child_dir).expect("create local package child");
    let resolved = resolve_local_param("file:..", &child_dir);

    assert_eq!(
        infer_local_package_alias(&resolved).expect("infer package alias"),
        format!("local-package@{resolved}"),
    );
}

#[test]
fn invalid_inferred_package_name_is_rejected() {
    let root = tempfile::tempdir().expect("create temp directory");
    let package_dir =
        create_local_package(root.path(), "local-package", r#"{ "name": "Invalid Name" }"#);
    let selector = format!("file:{}", package_dir.display());

    let error = infer_local_package_alias(&selector).expect_err("reject invalid package name");

    assert!(error.to_string().contains(r#"Invalid package name "Invalid Name"."#));
}

#[test]
fn pnpm_package_aliases_replace_each_other() {
    assert_eq!(replacement_aliases(&["@pnpm/exe".to_string()]), vec!["@pnpm/exe", "pnpm"]);
    assert_eq!(replacement_aliases(&["pnpm".to_string()]), vec!["pnpm", "@pnpm/exe"]);
}

#[test]
fn unrelated_aliases_are_not_expanded() {
    assert_eq!(
        replacement_aliases(&["eslint".to_string(), "typescript".to_string()]),
        vec!["eslint", "typescript"],
    );
}

#[test]
fn pnpm_alias_equivalence_only_replaces_pnpm_cli_groups() {
    let aliases = vec!["@pnpm/exe".to_string()];
    let aliases_to_replace = replacement_aliases(&aliases);

    assert!(should_replace_existing_package(
        &global_package(&["pnpm"]),
        &aliases,
        &aliases_to_replace,
    ));
    assert!(!should_replace_existing_package(
        &global_package(&["pnpm", "eslint"]),
        &aliases,
        &aliases_to_replace,
    ));
}

#[test]
fn exact_aliases_still_replace_mixed_groups() {
    let aliases = vec!["@pnpm/exe".to_string()];
    let aliases_to_replace = replacement_aliases(&aliases);

    assert!(should_replace_existing_package(
        &global_package(&["@pnpm/exe", "eslint"]),
        &aliases,
        &aliases_to_replace,
    ));
}

struct GlobalRemovalFixture {
    _root: TempDir,
    global_pkg_dir: PathBuf,
    global_bin_dir: PathBuf,
    group: GlobalPackageInfo,
    affected_bin_names: HashSet<String>,
}

#[derive(Clone, Copy)]
struct GlobalGroupSpec<'a> {
    alias: &'a str,
    hash: &'a str,
    bin: &'a str,
}

impl GlobalRemovalFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("create global removal fixture");
        let global_pkg_dir = root.path().join("global");
        let global_bin_dir = root.path().join("bin");
        let install_dir = global_pkg_dir.join("install");
        let package_dir = install_dir.join("node_modules/owner");
        fs::create_dir_all(&package_dir).expect("create installed package directory");
        fs::create_dir_all(&global_bin_dir).expect("create global bin directory");
        fs::write(
            package_dir.join("package.json"),
            serde_json::to_vec(&json!({
                "name": "owner",
                "version": "1.0.0",
                "bin": {
                    "owner": "owner.js",
                    "other": "other.js",
                },
            }))
            .expect("serialize installed package manifest"),
        )
        .expect("write installed package manifest");
        for name in ["owner", "other"] {
            fs::write(package_dir.join(format!("{name}.js")), "").expect("write bin source");
            fs::write(global_bin_dir.join(name), format!("old {name}\n"))
                .expect("write global bin");
        }
        let group = GlobalPackageInfo {
            hash: "group-hash".to_string(),
            install_dir,
            dependencies: vec![("owner".to_string(), "1.0.0".to_string())],
        };
        force_symlink_dir(
            &group.install_dir,
            &pnpm_global::get_hash_link(&global_pkg_dir, &group.hash),
        )
        .expect("seed global hash link");
        Self {
            _root: root,
            global_pkg_dir,
            global_bin_dir,
            group,
            affected_bin_names: HashSet::from(["owner".to_string(), "other".to_string()]),
        }
    }

    fn cleanup<'a>(&'a self, bins_to_keep: &'a HashSet<String>) -> GlobalInstallCleanup<'a> {
        GlobalInstallCleanup {
            global_pkg_dir: &self.global_pkg_dir,
            global_bin_dir: &self.global_bin_dir,
            bins_to_keep,
            hash_to_keep: None,
            context: "global",
        }
    }

    fn transaction<'a>(
        &'a self,
        cleanup: &'a GlobalInstallCleanup<'a>,
    ) -> GlobalRemovalTransaction<'a> {
        GlobalRemovalTransaction {
            groups: std::slice::from_ref(&self.group),
            cleanup,
            affected_bin_names: &self.affected_bin_names,
        }
    }

    fn seed_group(&self, spec: GlobalGroupSpec<'_>) -> GlobalPackageInfo {
        let install_dir = self.global_pkg_dir.join(format!("install-{}", spec.alias));
        let package_dir = install_dir.join("node_modules").join(spec.alias);
        fs::create_dir_all(&package_dir).expect("create installed package directory");
        fs::write(
            package_dir.join("package.json"),
            serde_json::to_vec(&json!({
                "name": spec.alias,
                "version": "1.0.0",
                "bin": { (spec.bin): format!("{}.js", spec.bin) },
            }))
            .expect("serialize installed package manifest"),
        )
        .expect("write installed package manifest");
        fs::write(package_dir.join(format!("{}.js", spec.bin)), "").expect("write bin source");
        fs::write(self.global_bin_dir.join(spec.bin), format!("old {}\n", spec.bin))
            .expect("write global bin");
        let group = GlobalPackageInfo {
            hash: spec.hash.to_string(),
            install_dir,
            dependencies: vec![(spec.alias.to_string(), "1.0.0".to_string())],
        };
        force_symlink_dir(
            &group.install_dir,
            &pnpm_global::get_hash_link(&self.global_pkg_dir, &group.hash),
        )
        .expect("seed global hash link");
        group
    }

    fn assert_package_commands_restored(&self) {
        assert_eq!(
            fs::read(self.global_bin_dir.join("owner")).expect("read owner bin"),
            b"old owner\n",
        );
        assert_eq!(
            fs::read(self.global_bin_dir.join("other")).expect("read other bin"),
            b"old other\n",
        );
        assert!(pnpm_global::get_hash_link(&self.global_pkg_dir, &self.group.hash).exists());
        assert!(self.group.install_dir.exists());
        let backup_count = fs::read_dir(&self.global_bin_dir)
            .expect("read global bin directory")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".pnpm-bin-backup-"))
            .count();
        assert_eq!(backup_count, 0);
    }
}

fn global_package(aliases: &[&str]) -> GlobalPackageInfo {
    GlobalPackageInfo {
        hash: "hash".to_string(),
        install_dir: PathBuf::from("/global/hash"),
        dependencies: aliases
            .iter()
            .map(|alias| ((*alias).to_string(), "1.0.0".to_string()))
            .collect(),
    }
}

fn create_local_package(root: &Path, directory_name: &str, manifest: &str) -> PathBuf {
    let package_dir = root.join(directory_name);
    std::fs::create_dir(&package_dir).expect("create local package");
    std::fs::write(package_dir.join("package.json"), manifest)
        .expect("write local package manifest");
    package_dir
}
