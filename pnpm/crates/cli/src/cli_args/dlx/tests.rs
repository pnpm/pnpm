use super::{DlxArgs, DlxError, get_bin_name, scopeless};
use crate::cli_args::dlx::{
    cache::{create_cache_key, get_prepare_dir, get_valid_cache_dir},
    clean::clean_expired_dlx_cache,
};
use clap::Parser;
use pnpm_fs::force_symlink_dir;
use pnpm_package_is_installable::{ArchitectureAxes, SupportedArchitectures};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tempfile::tempdir;

/// Parses `DlxArgs` as a flattened leaf so the architecture flags can be
/// exercised against the trailing `command` positional.
#[derive(Parser)]
struct DlxArgsWrapper {
    #[command(flatten)]
    dlx: DlxArgs,
}

#[test]
fn architecture_flags_do_not_consume_the_trailing_command() {
    let parsed = DlxArgsWrapper::try_parse_from([
        "dlx",
        "--cpu",
        "arm64,x64",
        "--os",
        "linux",
        "--libc",
        "musl",
        "cowsay",
        "hello",
    ])
    .expect("parse dlx args");

    assert_eq!(parsed.dlx.cpu, ["arm64", "x64"], "comma-separated --cpu values are split");
    assert_eq!(parsed.dlx.os, ["linux"]);
    assert_eq!(parsed.dlx.libc, ["musl"]);
    assert_eq!(parsed.dlx.command, ["cowsay", "hello"], "the command must survive after the flags");
}

#[test]
fn architecture_flags_accumulate_and_default_empty() {
    let parsed = DlxArgsWrapper::try_parse_from(["dlx", "--cpu", "arm64", "--cpu", "x64", "tool"])
        .expect("parse dlx args");

    assert_eq!(parsed.dlx.cpu, ["arm64", "x64"]);
    assert!(parsed.dlx.os.is_empty());
    assert!(parsed.dlx.libc.is_empty());
    assert_eq!(parsed.dlx.command, ["tool"]);
}

fn regs(default: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("default".to_string(), default.to_string());
    map
}

#[test]
fn create_cache_key_is_order_independent_and_deterministic() {
    let registry = "https://registry.npmjs.org/";
    let key_forward =
        create_cache_key(&["a".to_string(), "b".to_string()], &regs(registry), &[], None);
    let key_reversed =
        create_cache_key(&["b".to_string(), "a".to_string()], &regs(registry), &[], None);
    assert_eq!(key_forward, key_reversed, "the key must not depend on spec order");

    let key_versioned = create_cache_key(&["a@1".to_string()], &regs(registry), &[], None);
    assert_ne!(key_forward, key_versioned, "different specs must produce different keys");
}

#[test]
fn create_cache_key_depends_on_registry() {
    let pkgs = ["cowsay".to_string()];
    let key_default = create_cache_key(&pkgs, &regs("https://registry.npmjs.org/"), &[], None);
    let key_custom = create_cache_key(&pkgs, &regs("https://example.test/"), &[], None);
    assert_ne!(key_default, key_custom, "a different registry must produce a different key");
}

#[test]
fn create_cache_key_changes_with_allow_build() {
    let pkgs = ["cowsay".to_string()];
    let registry = "https://registry.npmjs.org/";
    let key_no_allow = create_cache_key(&pkgs, &regs(registry), &[], None);
    let key_with_allow = create_cache_key(&pkgs, &regs(registry), &["cowsay".to_string()], None);
    assert_ne!(key_no_allow, key_with_allow, "allow_build must change the key");
}

#[test]
fn create_cache_key_allow_build_is_order_independent() {
    let pkgs = ["cowsay".to_string()];
    let registry = "https://registry.npmjs.org/";
    let key_forward =
        create_cache_key(&pkgs, &regs(registry), &["a".to_string(), "b".to_string()], None);
    let key_reversed =
        create_cache_key(&pkgs, &regs(registry), &["b".to_string(), "a".to_string()], None);
    assert_eq!(key_forward, key_reversed, "allow_build order must not affect the key");
}

#[test]
fn create_cache_key_changes_with_supported_architectures() {
    let pkgs = ["cowsay".to_string()];
    let registry = "https://registry.npmjs.org/";
    let base = create_cache_key(&pkgs, &regs(registry), &[], None);

    let arm = SupportedArchitectures::Axes(ArchitectureAxes {
        cpu: Some(vec!["arm64".to_string()]),
        ..Default::default()
    });
    let x64 = SupportedArchitectures::Axes(ArchitectureAxes {
        cpu: Some(vec!["x64".to_string()]),
        ..Default::default()
    });
    let key_arm = create_cache_key(&pkgs, &regs(registry), &[], Some(&arm));
    let key_x64 = create_cache_key(&pkgs, &regs(registry), &[], Some(&x64));

    assert_ne!(base, key_arm, "an architecture override must change the key");
    assert_ne!(key_arm, key_x64, "different --cpu values must produce different keys");

    let arm_dup = SupportedArchitectures::Axes(ArchitectureAxes {
        cpu: Some(vec!["arm64".to_string(), "arm64".to_string()]),
        ..Default::default()
    });
    assert_eq!(
        key_arm,
        create_cache_key(&pkgs, &regs(registry), &[], Some(&arm_dup)),
        "duplicate cpu values must not change the key",
    );
}

#[test]
fn create_cache_key_changes_with_the_platforms_it_names() {
    let pkgs = ["cowsay".to_string()];
    let registry = "https://registry.npmjs.org/";
    let base = create_cache_key(&pkgs, &regs(registry), &[], None);
    let listed = |platforms: &[&str]| {
        SupportedArchitectures::Platforms(
            platforms
                .iter()
                .map(|platform| platform.parse().unwrap())
                .collect(),
        )
    };
    let key = |supported| create_cache_key(&pkgs, &regs(registry), &[], Some(supported));

    let linux = listed(&["linux-x64"]);
    let darwin = listed(&["darwin-arm64"]);
    assert_ne!(base, key(&linux), "naming a platform must change the key");
    assert_ne!(key(&linux), key(&darwin), "different platforms must produce different keys");

    let spelled_twice = listed(&["x86_64-unknown-linux-gnu", "linux-x64"]);
    assert_eq!(
        key(&linux),
        key(&spelled_twice),
        "two spellings of one platform must not change the key",
    );
    let written = listed(&["linux-x64", "darwin-arm64"]);
    let reversed = listed(&["darwin-arm64", "linux-x64"]);
    assert_ne!(
        key(&written),
        key(&reversed),
        "the first platform decides the runtime archive, so the order must change the key",
    );

    let here = listed(&["current"]);
    let elsewhere = listed(&["linux-arm64-musl"]);
    assert_ne!(base, key(&here), "the platform the install runs on must change the key");
    assert_ne!(
        key(&here),
        key(&elsewhere),
        "current must be recorded as the platform it resolves to, not as the word",
    );

    let axes = SupportedArchitectures::Axes(ArchitectureAxes {
        os: Some(vec!["linux".to_string()]),
        cpu: Some(vec!["x64".to_string()]),
        ..Default::default()
    });
    assert_ne!(
        key(&linux),
        key(&axes),
        "naming a platform and crossing the axes into it are different installs",
    );
}

#[test]
fn get_prepare_dir_encodes_time_and_pid_in_base36() {
    let base = std::path::Path::new("/cache/dlx/key");
    // 6699 = 5*36^2 + 6*36 + 3 -> "563"; 255 = 7*36 + 3 -> "73".
    let now = SystemTime::UNIX_EPOCH + Duration::from_millis(6699);
    let dir = get_prepare_dir(base, now, 255);
    assert_eq!(dir, base.join("563-73"));
}

#[test]
fn scopeless_strips_scope() {
    assert_eq!(scopeless("cowsay"), "cowsay");
    assert_eq!(scopeless("@scope/pkg"), "pkg");
    assert_eq!(scopeless("@scope"), "@scope");
}

#[test]
fn get_valid_cache_dir_is_none_for_missing_or_plain_dir() {
    let dir = tempdir().expect("temp dir");
    let missing = dir.path().join("pkg");
    assert!(get_valid_cache_dir(&missing, 1440, SystemTime::now()).is_none());

    let plain = dir.path().join("plain");
    fs::create_dir(&plain).expect("create plain dir");
    assert!(
        get_valid_cache_dir(&plain, 1440, SystemTime::now()).is_none(),
        "a non-symlink must not be treated as a valid cache",
    );
}

#[cfg(unix)]
#[test]
fn get_valid_cache_dir_honors_max_age() {
    let dir = tempdir().expect("temp dir");
    let target = dir.path().join("prepared");
    fs::create_dir(&target).expect("create target");
    let link = dir.path().join("pkg");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");

    let mtime = fs::symlink_metadata(&link)
        .expect("lstat")
        .modified()
        .expect("mtime");

    let within = mtime + Duration::from_secs(1440 * 60 - 1);
    assert_eq!(
        get_valid_cache_dir(&link, 1440, within).as_deref(),
        Some(fs::canonicalize(&target).expect("canonicalize").as_path()),
    );

    let past = mtime + Duration::from_mins(1441);
    assert!(get_valid_cache_dir(&link, 1440, past).is_none(), "an expired link must be rejected");
}

fn cache_entry(cache_dir: &Path, key: &str, prepare: &str) -> PathBuf {
    let prepare_dir = prepare_dir(cache_dir, key, prepare);
    force_symlink_dir(&prepare_dir, &entry_dir(cache_dir, key).join("pkg"))
        .expect("point pkg at the prepare dir");
    prepare_dir
}

fn prepare_dir(cache_dir: &Path, key: &str, prepare: &str) -> PathBuf {
    let prepare_dir = entry_dir(cache_dir, key).join(prepare);
    fs::create_dir_all(&prepare_dir).expect("create the prepare dir");
    prepare_dir
}

fn entry_dir(cache_dir: &Path, key: &str) -> PathBuf {
    cache_dir.join("dlx").join(key)
}

fn link_mtime(cache_dir: &Path, key: &str) -> SystemTime {
    fs::symlink_metadata(entry_dir(cache_dir, key).join("pkg"))
        .expect("lstat the pkg link")
        .modified()
        .expect("pkg link mtime")
}

fn entry_mtime(cache_dir: &Path, key: &str) -> SystemTime {
    fs::symlink_metadata(entry_dir(cache_dir, key))
        .expect("lstat the entry")
        .modified()
        .expect("entry mtime")
}

#[test]
fn clean_expired_dlx_cache_reclaims_an_entry_that_outlived_max_age() {
    let dir = tempdir().expect("temp dir");
    cache_entry(dir.path(), "key", "1-1");

    let now = link_mtime(dir.path(), "key") + Duration::from_mins(8);
    clean_expired_dlx_cache(dir.path(), 7, now).expect("clean the dlx cache");

    assert!(!dir.path().join("dlx/key").exists(), "the expired entry must be removed");
}

#[test]
fn clean_expired_dlx_cache_keeps_an_entry_inside_max_age() {
    let dir = tempdir().expect("temp dir");
    cache_entry(dir.path(), "key", "1-1");

    let now = link_mtime(dir.path(), "key");
    clean_expired_dlx_cache(dir.path(), 7, now).expect("clean the dlx cache");

    assert!(dir.path().join("dlx/key").exists(), "an entry inside max age must survive");
    assert!(dir.path().join("dlx/key/1-1").exists(), "the prepare dir it points at must survive");
}

#[test]
fn clean_expired_dlx_cache_keeps_an_entry_exactly_at_max_age() {
    let dir = tempdir().expect("temp dir");
    cache_entry(dir.path(), "key", "1-1");

    let now = link_mtime(dir.path(), "key") + Duration::from_mins(7);
    clean_expired_dlx_cache(dir.path(), 7, now).expect("clean the dlx cache");

    assert!(
        dir.path().join("dlx/key").exists(),
        "an entry exactly at dlxCacheMaxAge is still fresh",
    );
}

#[test]
fn clean_expired_dlx_cache_removes_every_entry_at_max_age_zero() {
    let dir = tempdir().expect("temp dir");
    cache_entry(dir.path(), "first", "1-1");
    cache_entry(dir.path(), "second", "2-2");
    let stray_file = dir.path().join("dlx/stray-file");
    fs::write(&stray_file, "noise").expect("write a file among the entries");

    let now = link_mtime(dir.path(), "first").max(link_mtime(dir.path(), "second"));
    clean_expired_dlx_cache(dir.path(), 0, now).expect("clean the dlx cache");

    assert!(!dir.path().join("dlx/first").exists(), "a zero max age expires every entry");
    assert!(!dir.path().join("dlx/second").exists(), "a zero max age expires every entry");
    assert!(stray_file.exists(), "a file directly under dlx is not a cache entry");
}

#[test]
fn clean_expired_dlx_cache_is_a_no_op_without_a_dlx_dir() {
    let dir = tempdir().expect("temp dir");

    clean_expired_dlx_cache(dir.path(), 7, SystemTime::now())
        .expect("a missing dlx dir is not an error");
}

#[test]
fn clean_expired_dlx_cache_errors_when_dlx_is_not_a_directory() {
    let dir = tempdir().expect("temp dir");
    fs::write(dir.path().join("dlx"), "not a directory").expect("write a dlx file");

    assert!(
        clean_expired_dlx_cache(dir.path(), 7, SystemTime::now()).is_err(),
        "a dlx path that cannot be enumerated must surface the error",
    );
}

#[test]
fn clean_expired_dlx_cache_does_not_follow_a_linked_dlx_root() {
    let dir = tempdir().expect("temp dir");
    let outside = tempdir().expect("outside temp dir");
    let outside_entry = outside.path().join("key").join("1-1");
    fs::create_dir_all(&outside_entry).expect("create the outside entry");
    force_symlink_dir(outside.path(), &dir.path().join("dlx")).expect("link dlx outside the cache");

    clean_expired_dlx_cache(dir.path(), 0, SystemTime::now()).expect("clean the dlx cache");

    assert!(outside_entry.exists(), "a linked dlx root must not lead the sweep outside the cache");
}

#[test]
fn clean_expired_dlx_cache_keeps_a_prepare_dir_that_has_no_link_yet() {
    let dir = tempdir().expect("temp dir");
    let prepare = prepare_dir(dir.path(), "key", "1-1");

    clean_expired_dlx_cache(dir.path(), 7, SystemTime::now()).expect("clean the dlx cache");

    assert!(prepare.exists(), "a prepare dir a concurrent run is still filling must survive");
}

#[test]
fn clean_expired_dlx_cache_reclaims_a_linkless_entry_once_it_outlives_max_age() {
    let dir = tempdir().expect("temp dir");
    let prepare = prepare_dir(dir.path(), "key", "1-1");

    let now = entry_mtime(dir.path(), "key") + Duration::from_mins(8);
    clean_expired_dlx_cache(dir.path(), 7, now).expect("clean the dlx cache");

    assert!(!prepare.exists(), "a linkless entry that outlived max age must go");
}

#[test]
fn clean_expired_dlx_cache_keeps_a_replacement_prepare_dir_while_the_link_is_stale() {
    let dir = tempdir().expect("temp dir");
    let stale = cache_entry(dir.path(), "key", "1-1");
    // The replacement appears after the link was last repointed, so the link
    // is expired while the entry itself is not.
    std::thread::sleep(Duration::from_secs(1));
    let replacement = prepare_dir(dir.path(), "key", "2-2");

    let now = link_mtime(dir.path(), "key") + Duration::from_mins(7) + Duration::from_millis(500);
    clean_expired_dlx_cache(dir.path(), 7, now).expect("clean the dlx cache");

    assert!(dir.path().join("dlx/key").exists(), "the entry must survive while a run replaces it");
    assert!(stale.exists(), "the target pkg still points at must survive");
    assert!(replacement.exists(), "the replacement prepare dir must survive");
}

#[test]
fn clean_expired_dlx_cache_reclaims_a_superseded_prepare_dir_once_it_outlives_max_age() {
    let dir = tempdir().expect("temp dir");
    let superseded = prepare_dir(dir.path(), "key", "1-1");
    std::thread::sleep(Duration::from_secs(1));
    let current = cache_entry(dir.path(), "key", "2-2");

    let now = link_mtime(dir.path(), "key") + Duration::from_mins(7);
    clean_expired_dlx_cache(dir.path(), 7, now).expect("clean the dlx cache");

    assert!(current.exists(), "the prepare dir pkg points at must survive");
    assert!(!superseded.exists(), "a superseded prepare dir that outlived max age must go");
}

#[test]
fn clean_expired_dlx_cache_reclaims_an_entry_whose_pkg_is_not_a_symlink() {
    let dir = tempdir().expect("temp dir");
    let prepare = prepare_dir(dir.path(), "key", "1-1");
    fs::write(entry_dir(dir.path(), "key").join("pkg"), "not a link").expect("write a pkg file");

    let now = entry_mtime(dir.path(), "key") + Duration::from_mins(8);
    clean_expired_dlx_cache(dir.path(), 7, now).expect("clean the dlx cache");

    assert!(!prepare.exists(), "an entry whose pkg is not a symlink must be reclaimed");
}

#[test]
fn clean_expired_dlx_cache_keeps_fresh_orphans_under_a_broken_link() {
    let dir = tempdir().expect("temp dir");
    let gone = cache_entry(dir.path(), "key", "1-1");
    fs::remove_dir_all(&gone).expect("remove the link target");
    let orphan = prepare_dir(dir.path(), "key", "2-2");

    clean_expired_dlx_cache(dir.path(), 7, SystemTime::now()).expect("clean the dlx cache");

    assert!(
        fs::symlink_metadata(entry_dir(dir.path(), "key").join("pkg")).is_ok(),
        "the broken link must survive",
    );
    assert!(orphan.exists(), "a fresh prepare dir must survive a broken link");
}

#[cfg(unix)]
fn with_mode<Output>(dir: &Path, mode: u32, body: impl FnOnce() -> Output) -> Output {
    use std::os::unix::fs::PermissionsExt;
    let original = fs::metadata(dir).expect("stat the directory").permissions();
    fs::set_permissions(dir, fs::Permissions::from_mode(mode)).expect("set the directory mode");
    let result = body();
    fs::set_permissions(dir, original).expect("restore the directory mode");
    result
}

#[cfg(unix)]
#[test]
fn clean_expired_dlx_cache_surfaces_a_removal_failure() {
    let dir = tempdir().expect("temp dir");
    cache_entry(dir.path(), "key", "1-1");
    let now = link_mtime(dir.path(), "key") + Duration::from_mins(8);

    let result = with_mode(&entry_dir(dir.path(), "key"), 0o555, || {
        clean_expired_dlx_cache(dir.path(), 7, now)
    });

    assert!(result.is_err(), "a directory that cannot be removed must surface the error");
}

#[cfg(unix)]
#[test]
fn clean_expired_dlx_cache_surfaces_a_pkg_stat_failure() {
    let dir = tempdir().expect("temp dir");
    cache_entry(dir.path(), "key", "1-1");

    let result = with_mode(&entry_dir(dir.path(), "key"), 0o000, || {
        clean_expired_dlx_cache(dir.path(), 7, SystemTime::now())
    });

    assert!(result.is_err(), "an unreadable pkg link must surface the error");
}

#[cfg(unix)]
#[test]
fn clean_expired_dlx_cache_surfaces_an_unsearchable_dlx_dir() {
    let dir = tempdir().expect("temp dir");
    cache_entry(dir.path(), "key", "1-1");

    let result = with_mode(&dir.path().join("dlx"), 0o444, || {
        clean_expired_dlx_cache(dir.path(), 7, SystemTime::now())
    });

    assert!(result.is_err(), "an unsearchable dlx dir must surface the error");
}

#[cfg(unix)]
#[test]
fn clean_expired_dlx_cache_surfaces_an_orphan_removal_failure() {
    let dir = tempdir().expect("temp dir");
    let orphan = prepare_dir(dir.path(), "key", "1-1");
    std::thread::sleep(Duration::from_secs(1));
    cache_entry(dir.path(), "key", "2-2");
    let now = link_mtime(dir.path(), "key") + Duration::from_mins(7);

    let result = with_mode(&entry_dir(dir.path(), "key"), 0o555, || {
        clean_expired_dlx_cache(dir.path(), 7, now)
    });

    assert!(result.is_err(), "an orphan that cannot be removed must surface the error");
    assert!(orphan.exists(), "the orphan must survive when its removal fails");
}

#[test]
fn get_valid_cache_dir_treats_a_link_from_the_future_as_fresh() {
    let dir = tempdir().expect("temp dir");
    let prepare = prepare_dir(dir.path(), "key", "1-1");
    let link = entry_dir(dir.path(), "key").join("pkg");
    force_symlink_dir(&prepare, &link).expect("point pkg at the prepare dir");

    let before = link_mtime(dir.path(), "key") - Duration::from_secs(1);
    assert!(
        get_valid_cache_dir(&link, 7, before).is_some(),
        "a clock that reads before the link's mtime must not expire it",
    );
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn write_pkg(dir: &std::path::Path, name: &str, manifest: serde_json::Value) {
    let pkg_dir = dir.join("node_modules").join(name);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write pkg manifest");
}

fn cached_dir_with(dep: &str, manifest: serde_json::Value) -> tempfile::TempDir {
    let dir = tempdir().expect("temp dir");
    fs::write(
        dir.path().join("package.json"),
        serde_json::json!({ "dependencies": { dep: "1.0.0" } }).to_string(),
    )
    .expect("write root manifest");
    write_pkg(dir.path(), dep, manifest);
    dir
}

#[test]
fn get_bin_name_returns_single_bin() {
    let dir = cached_dir_with(
        "cowsay",
        serde_json::json!({ "name": "cowsay", "version": "1.0.0", "bin": { "cowsay": "cli.js" } }),
    );
    assert_eq!(get_bin_name(dir.path()).expect("bin name"), "cowsay");
}

#[test]
fn get_bin_name_picks_the_scopeless_match_among_many() {
    let dir = cached_dir_with(
        "@scope/tool",
        serde_json::json!({
            "name": "@scope/tool",
            "version": "1.0.0",
            "bin": { "tool": "tool.js", "other": "other.js" },
        }),
    );
    assert_eq!(get_bin_name(dir.path()).expect("bin name"), "tool");
}

#[test]
fn get_bin_name_uses_installed_manifest_name_not_alias() {
    let dir = cached_dir_with(
        "alias",
        serde_json::json!({
            "name": "@scope/realtool",
            "version": "1.0.0",
            "bin": { "realtool": "r.js", "other": "o.js" },
        }),
    );
    assert_eq!(get_bin_name(dir.path()).expect("bin name"), "realtool");
}

#[test]
fn get_bin_name_errors_when_no_dependency() {
    let dir = tempdir().expect("temp dir");
    fs::write(dir.path().join("package.json"), serde_json::json!({}).to_string())
        .expect("write manifest");
    assert!(matches!(get_bin_name(dir.path()), Err(DlxError::NoDep)));
}

#[test]
fn get_bin_name_errors_on_ambiguous_bins() {
    let dir = cached_dir_with(
        "multi",
        serde_json::json!({
            "name": "multi",
            "version": "1.0.0",
            "bin": { "one": "one.js", "two": "two.js" },
        }),
    );
    assert!(matches!(get_bin_name(dir.path()), Err(DlxError::MultipleBins { .. })));
}

/// A dlx-installed runtime (`pnpm dlx node@runtime:<version>`) is recorded
/// in the cache manifest as `engines.runtime` by the manifest writer's
/// round-trip, not under `dependencies` — the installed package must still
/// be found there.
#[test]
fn get_bin_name_finds_a_runtime_recorded_as_engines_runtime() {
    let dir = tempdir().expect("temp dir");
    fs::write(
        dir.path().join("package.json"),
        serde_json::json!({
            "name": "dlx",
            "version": "0.0.0",
            "dependencies": {},
            "engines": {
                "runtime": { "name": "node", "version": "26.4.0", "onFail": "download" },
            },
        })
        .to_string(),
    )
    .expect("write root manifest");
    write_pkg(
        dir.path(),
        "node",
        serde_json::json!({ "name": "node", "version": "26.4.0", "bin": { "node": "bin/node" } }),
    );
    assert_eq!(get_bin_name(dir.path()).expect("bin name"), "node");
}

/// The command word decides whether dlx provisions a tool or installs a
/// package. Only the names pnpm actually manages are routed away from the
/// ordinary path.
#[test]
fn only_managed_tools_are_provisioned_by_name() {
    use super::provision::{parse_package_manager_spec, parse_runtime_spec};
    use crate::engine_pm::channel::PackageManager;

    assert_eq!(parse_package_manager_spec("yarn@4"), Some((PackageManager::Yarn, "4")));
    assert_eq!(parse_package_manager_spec("npm"), Some((PackageManager::Npm, "latest")));
    assert_eq!(parse_package_manager_spec("typescript@5"), None);
    // A scoped package's leading `@` is not a version separator.
    assert_eq!(parse_package_manager_spec("@yarnpkg/cli-dist@4.9.2"), None);

    assert_eq!(parse_runtime_spec("node@22"), Some(("node", "22")));
    assert_eq!(parse_runtime_spec("deno"), Some(("deno", "latest")));
    assert_eq!(parse_runtime_spec("nodemon@3"), None);
    assert_eq!(parse_runtime_spec("node@runtime:22"), Some(("node", "22")));

    // A specifier that locates a package names what to install, whether it
    // spells out a protocol or uses the GitHub shorthand.
    assert_eq!(parse_package_manager_spec("yarn@npm:@yarnpkg/cli-dist@4.9.2"), None);
    assert_eq!(parse_package_manager_spec("yarn@yarnpkg/berry"), None);
    assert_eq!(parse_package_manager_spec("yarn@yarnpkg/berry#main"), None);
    assert_eq!(parse_runtime_spec("node@github:nodejs/node"), None);
    assert_eq!(parse_runtime_spec("node@nodejs/node"), None);

    // `--package` names the engine, and the command names which of its
    // bins to run: every one the channel table publishes qualifies.
    assert!(PackageManager::Npm.bins().contains(&"npx"));
    assert!(PackageManager::Yarn.bins().contains(&"yarnpkg"));
    assert!(!PackageManager::Npm.bins().contains(&"yarn"));
}

#[cfg(windows)]
#[test]
fn clean_expired_dlx_cache_does_not_follow_a_junction_root() {
    let dir = tempdir().expect("temp dir");
    let outside = tempdir().expect("outside temp dir");
    let outside_entry = outside.path().join("key").join("1-1");
    fs::create_dir_all(&outside_entry).expect("create the outside entry");
    let output = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(dir.path().join("dlx"))
        .arg(outside.path())
        .output()
        .expect("create a junction");
    assert!(output.status.success(), "mklink failed: {output:?}");

    clean_expired_dlx_cache(dir.path(), 0, SystemTime::now()).expect("clean the dlx cache");

    assert!(outside_entry.exists(), "a junction root must not lead cleanup outside the cache");
}
