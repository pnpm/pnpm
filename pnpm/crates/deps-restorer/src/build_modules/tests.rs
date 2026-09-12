mod workspace;

mod links;

mod frozen;

mod resolution;

mod integrity;

mod store;

mod installation;

mod reporting;

mod patches_and_approvals;

mod build_policy;

use super::{BuildModules, allow_build_policy::AllowBuildPolicy};
// Only the `#[cfg(unix)]` rebuild-selection test uses this; importing it
// unconditionally would be an unused import on Windows.
use crate::{SkippedSnapshots, VirtualStoreLayout};
use pnpm_config::{Config, PackageImportMethod};
use pnpm_executor::ScriptsPrependNodePath;
use pnpm_lockfile::{
    PackageKey, PkgName, PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec, SnapshotEntry,
};
use pnpm_reporter::SilentReporter;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

/// Install-scoped `pnpm:package-import-method` dedupe state shared by
/// the `BuildModules` constructions below. The build phase only writes
/// to it through the side-effects re-materialization path; tests don't
/// assert on it, so one shared static is enough.
static TEST_LOGGED_METHODS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Build an [`AllowBuildPolicy`] from a list of `(spec, allowed)`
/// pairs, mirroring how `pnpm-workspace.yaml`'s `allowBuilds` map
/// would arrive at the policy. Each spec is parsed through
/// [`AllowBuildPolicy::from_config`] so version unions and depPath
/// keys work the same way they do at runtime.
/// Panics on any parse failure — test inputs must be valid.
fn policy_from_specs<const LEN: usize>(
    entries: [(&str, bool); LEN],
    dangerously_allow_all: bool,
) -> AllowBuildPolicy {
    let mut config = Config::new();
    config.dangerously_allow_all_builds = dangerously_allow_all;
    for (spec, value) in entries {
        config.allow_builds.insert(spec.to_string(), value);
    }
    AllowBuildPolicy::from_config(&config).expect("valid specs")
}

fn name(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse pkg name")
}

fn ver(text: &str) -> PkgVerPeer {
    text.parse().expect("parse PkgVerPeer")
}

fn key(name_text: &str, version: &str) -> PackageKey {
    PackageKey::new(name(name_text), ver(version))
}

/// Materialize a `<virtual_store_dir>/<store_name>/node_modules/<pkg_name>/package.json`
/// fixture so `pkg_requires_build` returns true for `key`. The script body
/// is harmless (`true`) — the existing tests rely on the policy gate to block
/// execution, so the script never actually runs.
fn create_buildable_pkg(virtual_store_dir: &Path, key: &PackageKey) -> PathBuf {
    let key_str = key.without_peer().to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);
    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let pkg_name = &name_version[..at_idx];
    let store_name = name_version.replace('/', "+");
    let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    let manifest = serde_json::json!({
        "scripts": { "postinstall": "true" },
    });
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write manifest");
    pkg_dir
}

fn root_importers(deps: &[(&str, &str)]) -> HashMap<String, ProjectSnapshot> {
    let map: ResolvedDependencyMap = deps
        .iter()
        .map(|(n, v)| {
            (
                name(n),
                ResolvedDependencySpec { specifier: (*v).to_string(), version: ver(v).into() },
            )
        })
        .collect();
    HashMap::from([(
        ".".to_string(),
        ProjectSnapshot {
            specifiers: None,
            dependencies: (!map.is_empty()).then_some(map),
            optional_dependencies: None,
            dev_dependencies: None,
            dependencies_meta: None,
            publish_directory: None,
            link_directory: None,
        },
    )])
}

// --- frozen-store build backstop ---------------------------------------

/// A `pnpm-workspace.yaml`-shaped patch entry for one package. The
/// `patch_file_path` is left `None` because the frozen-store backstop
/// fires before any patch application — the entry only has to make
/// `has_patch` true and land the snapshot in the build sequence.
fn single_patch(key: &PackageKey) -> HashMap<PackageKey, pnpm_patching::ExtendedPatchInfo> {
    HashMap::from([(
        key.without_peer(),
        pnpm_patching::ExtendedPatchInfo {
            hash: "deadbeef".to_string(),
            patch_file_path: None,
            key: key.without_peer().to_string(),
        },
    )])
}

/// Build a global-virtual-store [`VirtualStoreLayout`] for a backstop
/// test. `snapshots: None` short-circuits [`VirtualStoreLayout::new`]
/// to an empty `gvs_suffixes` map, which is all the backstop needs —
/// it queries [`VirtualStoreLayout::enable_global_virtual_store`]
/// (true iff `gvs_suffixes.is_some()`) and fires before any slot-dir
/// lookup.
fn gvs_layout(dir: &Path) -> &'static VirtualStoreLayout {
    let mut config = Config::new();
    config.enable_global_virtual_store = true;
    config.store_dir = dir.join("store").into();
    config.global_virtual_store_dir = dir.join("store/links");
    config.virtual_store_dir = dir.join("node_modules/.pacquet");
    let config = config.leak();
    Box::leak(Box::new(VirtualStoreLayout::new(config, None, None, None, None, None)))
}

/// Run [`BuildModules`] over a single patched `is-positive@1.0.0`
/// snapshot, varying only `layout`, `frozen_store`, and the snapshot's
/// `optional` flag — the three backstop inputs. No on-disk fixture: the
/// snapshot's slot never materializes, so on paths that skip the
/// backstop the run no-ops at the `!pkg_dir.exists()` guard rather than
/// touching the (absent) patch file.
fn frozen_backstop_run(
    layout: &VirtualStoreLayout,
    frozen_store: bool,
    optional: bool,
) -> Result<crate::BuildModulesOutput, crate::build_modules::BuildModulesError> {
    let pkg_key = key("is-positive", "1.0.0");
    let snapshots =
        HashMap::from([(pkg_key.clone(), SnapshotEntry { optional, ..SnapshotEntry::default() })]);
    let patches = single_patch(&pkg_key);
    let importers = root_importers(&[("is-positive", "1.0.0")]);
    let policy = policy_from_specs([], false);
    let modules_dir = tempdir().expect("create temp dir");
    let lockfile_dir = tempdir().expect("create temp dir");

    BuildModules {
        layout,
        modules_dir: modules_dir.path(),
        lockfile_dir: lockfile_dir.path(),
        snapshots: Some(&snapshots),
        packages: None,
        importers: &importers,
        allow_build_policy: &policy,
        side_effects_maps_by_snapshot: None,
        requires_build_by_snapshot: None,
        engine_name: None,
        side_effects_cache: false,
        side_effects_cache_write: false,
        shared_side_effects_publisher: None,
        store_dir: None,
        store_index_writer: None,
        patches: Some(&patches),

        scripts_prepend_node_path: ScriptsPrependNodePath::Never,

        script_shell: None,
        shell_emulator: false,
        extra_env: &HashMap::new(),
        user_agent: "pnpm/test",
        unsafe_perm: true,
        child_concurrency: 1,
        skipped: &SkippedSnapshots::default(),
        pkg_roots_by_key: None,
        gather_ancestor_bin_paths: false,
        frozen_store,
        ignore_scripts: false,
        import_method: PackageImportMethod::Auto,
        logged_methods: &TEST_LOGGED_METHODS,
        rebuild: None,
    }
    .run::<SilentReporter>()
}

/// Materialize a package fixture matching the
/// `@pnpm.e2e/failing-postinstall@1.0.0` registry-mock package. The
/// script body (`echo hello && echo world && exit 1`) reproduces the
/// failure mode and exit code without dragging the
/// lockfile-with-real-integrity machinery into a `BuildModules`-unit
/// test.
#[cfg(unix)]
fn create_failing_postinstall_fixture(virtual_store_dir: &Path, key: &PackageKey) -> PathBuf {
    let key_str = key.without_peer().to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);
    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let pkg_name = &name_version[..at_idx];
    let store_name = name_version.replace('/', "+");
    let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    let manifest = serde_json::json!({
        "name": pkg_name,
        "version": name_version[at_idx + 1..].to_string(),
        "scripts": { "postinstall": "echo hello && echo world && exit 1" },
    });
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write manifest");
    pkg_dir
}

/// Materialize a package fixture whose postinstall touches a
/// marker file. After the script runs, the package directory has
/// a file (`generated.txt`) that the original tarball didn't, so
/// the WRITE-path diff produces a non-empty `added` entry under
/// the snapshot's cache key.
///
/// Returns the package directory path and the actual file mode of
/// `index.js`. The mode is read from disk because `fs::write()`
/// assigns permissions according to the process umask (typically
/// 0022 → 0o644, but 0002 → 0o664 when `pam_umask`'s `usergroups`
/// logic matches UID to group name, the default on Debian for
/// non-root users). Callers that pre-seed store rows should use
/// this returned mode so `calculate_diff()` doesn't flag a
/// mode-only mismatch on unchanged files.
#[cfg(unix)]
fn create_postinstall_modifies_source_fixture(
    virtual_store_dir: &Path,
    key: &PackageKey,
) -> (PathBuf, u32) {
    use std::os::unix::fs::PermissionsExt;

    let key_str = key.without_peer().to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);
    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let pkg_name = &name_version[..at_idx];
    let store_name = name_version.replace('/', "+");
    let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    // Bake the pristine `index.js` into the directory before the
    // postinstall runs. The WRITE-path diff compares the
    // post-build directory against the pre-seeded `files` map,
    // so this file must appear in both — the postinstall only
    // adds `generated.txt` on top.
    fs::write(pkg_dir.join("index.js"), "module.exports = 'hi'\n").expect("write index.js");
    let manifest = serde_json::json!({
        "name": pkg_name,
        "version": name_version[at_idx + 1..].to_string(),
        "scripts": { "postinstall": "echo touched > generated.txt" },
    });
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write manifest");
    let actual_mode =
        std::fs::metadata(pkg_dir.join("index.js")).expect("stat index.js").permissions().mode()
            & 0o777;
    (pkg_dir, actual_mode)
}

fn report_store_file_differences(
    before: &std::collections::BTreeMap<PathBuf, Vec<u8>>,
    after: &std::collections::BTreeMap<PathBuf, Vec<u8>>,
) {
    if after == before {
        return;
    }
    eprintln!("Store regular files differ:");
    for (path, contents) in after {
        if before.get(path) != Some(contents) {
            eprintln!("  added or modified: {}", path.display());
        }
    }
    for path in before.keys() {
        if !after.contains_key(path) {
            eprintln!("  removed: {}", path.display());
        }
    }
}

/// Variant of [`create_postinstall_modifies_source_fixture`] whose
/// postinstall additionally produces a 0-permission file. The
/// WRITE-path walker (`add_files_from_dir`) then fails on
/// `fs::read("unreadable")` with `EACCES`, surfacing as
/// `UploadError::AddFilesFromDir(ReadFile { … })` — a real upload
/// error that `BuildModules` must swallow.
#[cfg(unix)]
fn create_postinstall_with_unreadable_fixture(
    virtual_store_dir: &Path,
    key: &PackageKey,
) -> PathBuf {
    let key_str = key.without_peer().to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);
    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let pkg_name = &name_version[..at_idx];
    let store_name = name_version.replace('/', "+");
    let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    fs::write(pkg_dir.join("index.js"), "module.exports = 'hi'\n").expect("write index.js");
    let manifest = serde_json::json!({
        "name": pkg_name,
        "version": name_version[at_idx + 1..].to_string(),
        "scripts": {
            "postinstall": "echo touched > generated.txt && : > unreadable && chmod 000 unreadable"
        },
    });
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write manifest");
    pkg_dir
}

/// sha-512 hex helper for fixture-building. Pacquet's `CafsFileInfo`
/// stores digests as raw hex (no `sha512-` prefix); using the same
/// shape here keeps the test's pre-seeded base row in lockstep with
/// what `add_files_from_dir` will compute.
#[cfg(unix)]
fn sha512_hex(buf: &[u8]) -> String {
    use sha2::{Digest, Sha512};
    let digest = Sha512::digest(buf);
    format!("{digest:x}")
}

#[cfg(unix)]
fn snapshot_regular_files(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut snapshot = std::collections::BTreeMap::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory).expect("read snapshot directory") {
            let entry = entry.expect("read snapshot entry");
            let file_type = entry.file_type().expect("read snapshot entry type");
            if file_type.is_dir() {
                directories.push(entry.path());
            } else if file_type.is_file() {
                let path = entry.path();
                let relative = path.strip_prefix(root).expect("snapshot path is under root");
                snapshot
                    .insert(relative.to_path_buf(), fs::read(&path).expect("read snapshot file"));
            }
        }
    }
    snapshot
}

/// Like [`create_buildable_pkg`], but the postinstall writes a `built-marker`
/// file into the package directory so a test can observe whether the script
/// actually ran.
#[cfg(unix)]
fn create_marker_pkg(virtual_store_dir: &Path, key: &PackageKey) -> PathBuf {
    let key_str = key.without_peer().to_string();
    let name_version = key_str.strip_prefix('/').unwrap_or(&key_str);
    let at_idx = name_version.rfind('@').unwrap_or(name_version.len());
    let pkg_name = &name_version[..at_idx];
    let store_name = name_version.replace('/', "+");
    let pkg_dir = virtual_store_dir.join(&store_name).join("node_modules").join(pkg_name);
    fs::create_dir_all(&pkg_dir).expect("create pkg dir");
    let manifest = serde_json::json!({
        "scripts": { "postinstall": "echo ran > built-marker" },
    });
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write manifest");
    pkg_dir
}
