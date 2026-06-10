use super::{
    DlxArgs, DlxError, create_cache_key, get_bin_name, get_prepare_dir, get_valid_cache_dir,
    scopeless,
};
use clap::Parser;
use pacquet_package_is_installable::SupportedArchitectures;
use std::{
    collections::BTreeMap,
    fs,
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

/// The `--cpu` / `--os` / `--libc` overrides take one comma-separable
/// value per occurrence, so the trailing `command` positional is not
/// swallowed as extra architecture values.
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

/// Repeated `--cpu` occurrences accumulate, and an absent axis stays
/// empty (so it leaves the config value untouched downstream).
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

    let arm = SupportedArchitectures { cpu: Some(vec!["arm64".to_string()]), ..Default::default() };
    let x64 = SupportedArchitectures { cpu: Some(vec!["x64".to_string()]), ..Default::default() };
    let key_arm = create_cache_key(&pkgs, &regs(registry), &[], Some(&arm));
    let key_x64 = create_cache_key(&pkgs, &regs(registry), &[], Some(&x64));

    assert_ne!(base, key_arm, "an architecture override must change the key");
    assert_ne!(key_arm, key_x64, "different --cpu values must produce different keys");

    // Dedup + sort make the axis stable: order and duplicates don't matter.
    let arm_dup = SupportedArchitectures {
        cpu: Some(vec!["arm64".to_string(), "arm64".to_string()]),
        ..Default::default()
    };
    assert_eq!(
        key_arm,
        create_cache_key(&pkgs, &regs(registry), &[], Some(&arm_dup)),
        "duplicate cpu values must not change the key",
    );
}

#[test]
fn get_prepare_dir_encodes_time_and_pid_in_hex() {
    let base = std::path::Path::new("/cache/dlx/key");
    let now = SystemTime::UNIX_EPOCH + Duration::from_millis(0x1a2b);
    let dir = get_prepare_dir(base, now, 0xff);
    assert_eq!(dir, base.join("1a2b-ff"));
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

    let mtime = fs::symlink_metadata(&link).expect("lstat").modified().expect("mtime");

    // Just under the window: still valid.
    let within = mtime + Duration::from_secs(1440 * 60 - 1);
    assert_eq!(
        get_valid_cache_dir(&link, 1440, within).as_deref(),
        Some(fs::canonicalize(&target).expect("canonicalize").as_path()),
    );

    // Past the window: expired.
    let past = mtime + Duration::from_mins(1441);
    assert!(get_valid_cache_dir(&link, 1440, past).is_none(), "an expired link must be rejected");
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
    // The dependency key (`alias`) differs from the installed package's
    // own `name`. The default bin among many is selected by the manifest
    // name (`scopeless("@scope/realtool")` == "realtool"), not the alias.
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
