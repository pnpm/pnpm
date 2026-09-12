use super::{
    PEER_HEAVY_DEPTH, PEER_HEAVY_INTEGRITY, PEER_HEAVY_PROVIDER, PEER_HEAVY_VERSION,
    PEER_HEAVY_WIDTH,
};
use crate::{
    cli_args::BenchmarkScenario, fixtures::PACKAGE_JSON,
    workspace_manifest::MinimalWorkspaceManifest,
};
use serde_json::Value;
use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};

pub(super) fn create_package_json(
    dst_dir: &Path,
    src_dir: Option<&Path>,
    scenario: BenchmarkScenario,
) {
    let dst = dst_dir.join("package.json");
    if scenario.uses_peer_heavy_fixture() {
        fs::write(dst, peer_heavy_root_manifest()).expect("write peer-heavy package.json");
        return;
    }
    if let Some(src_dir) = src_dir {
        let src = src_dir.join("package.json");
        assert!(src.is_file(), "{src:?} must be a file");
        assert_ne!(src, dst);
        fs::copy(src, dst).expect("copy package.json for the revision");
    } else {
        fs::write(dst, PACKAGE_JSON).expect("write package.json for the revision");
    }
}
pub(super) fn peer_heavy_root_manifest() -> String {
    let mut dependencies = serde_json::Map::new();
    dependencies
        .insert(PEER_HEAVY_PROVIDER.to_string(), Value::String(PEER_HEAVY_VERSION.to_string()));
    for index in 0..PEER_HEAVY_WIDTH {
        dependencies.insert(
            peer_heavy_package_name(0, index),
            Value::String(PEER_HEAVY_VERSION.to_string()),
        );
    }
    serde_json::to_string_pretty(&serde_json::json!({
        "name": "peer-heavy-benchmark-root",
        "version": "0.0.0",
        "private": true,
        "dependencies": dependencies,
    }))
    .expect("serialize peer-heavy root manifest")
}
pub(crate) fn seed_peer_heavy_registry(storage_root: &Path) {
    write_peer_heavy_packument(storage_root, PEER_HEAVY_PROVIDER, &serde_json::Map::new(), false);
    for level in 0..PEER_HEAVY_DEPTH {
        let dependencies = if level + 1 == PEER_HEAVY_DEPTH {
            serde_json::Map::new()
        } else {
            (0..PEER_HEAVY_WIDTH)
                .map(|index| {
                    (
                        peer_heavy_package_name(level + 1, index),
                        Value::String(PEER_HEAVY_VERSION.to_string()),
                    )
                })
                .collect()
        };
        for index in 0..PEER_HEAVY_WIDTH {
            write_peer_heavy_packument(
                storage_root,
                &peer_heavy_package_name(level, index),
                &dependencies,
                true,
            );
        }
    }
}
pub(super) fn write_peer_heavy_packument(
    storage_root: &Path,
    name: &str,
    dependencies: &serde_json::Map<String, Value>,
    has_peer: bool,
) {
    let manifest = peer_heavy_manifest(name, dependencies, has_peer);
    let versions = serde_json::Map::from_iter([(PEER_HEAVY_VERSION.to_string(), manifest)]);
    let time = serde_json::Map::from_iter([
        ("created".to_string(), Value::String("2020-01-01T00:00:00.000Z".to_string())),
        ("modified".to_string(), Value::String("2020-01-01T00:00:00.000Z".to_string())),
        (PEER_HEAVY_VERSION.to_string(), Value::String("2020-01-01T00:00:00.000Z".to_string())),
    ]);
    let packument = serde_json::json!({
        "name": name,
        "dist-tags": { "latest": PEER_HEAVY_VERSION },
        "versions": versions,
        "time": time,
    });
    let package_dir = storage_root.join(name);
    fs::create_dir_all(&package_dir).expect("create peer-heavy registry package directory");
    fs::write(
        package_dir.join("package.json"),
        serde_json::to_vec(&packument).expect("serialize peer-heavy packument"),
    )
    .expect("write peer-heavy packument");
}
pub(super) fn peer_heavy_manifest(
    name: &str,
    dependencies: &serde_json::Map<String, Value>,
    has_peer: bool,
) -> Value {
    let mut manifest = serde_json::json!({
        "name": name,
        "version": PEER_HEAVY_VERSION,
        "dependencies": dependencies,
        "dist": {
            "tarball": format!(
                "http://example.test/{name}/-/{}-{PEER_HEAVY_VERSION}.tgz",
                name.rsplit('/').next().expect("package name has a final segment"),
            ),
            "integrity": PEER_HEAVY_INTEGRITY,
        },
    });
    if has_peer {
        let peer_dependencies = serde_json::Map::from_iter([(
            PEER_HEAVY_PROVIDER.to_string(),
            Value::String(PEER_HEAVY_VERSION.to_string()),
        )]);
        manifest
            .as_object_mut()
            .expect("package manifest is an object")
            .insert("peerDependencies".to_string(), Value::Object(peer_dependencies));
    }
    manifest
}
pub(super) fn peer_heavy_package_name(level: usize, index: usize) -> String {
    format!("@pnpmtest/peer-benchmark-level-{level}-{index:02}")
}
/// Save pristine copies of `package.json` and (when present)
/// `pnpm-lock.yaml` next to the originals. The cleanup phase between
/// hyperfine iterations restores from these so a mutating install
/// (`add`, `--no-frozen-lockfile`) doesn't drift the project state
/// across runs.
pub(super) fn save_pristine_copies(dir: &Path) {
    let pkg = dir.join("package.json");
    if pkg.is_file() {
        fs::copy(&pkg, dir.join(".saved-package.json")).expect("save pristine package.json");
    }
    let lock = dir.join("pnpm-lock.yaml");
    if lock.is_file() {
        fs::copy(&lock, dir.join(".saved-pnpm-lock.yaml")).expect("save pristine pnpm-lock.yaml");
    }
}
/// Synthesize the per-revision `pnpm-workspace.yaml` through a typed
/// [`MinimalWorkspaceManifest`] and emit it via `serde_saphyr`, instead
/// of formatting raw YAML strings. The typed round-trip rules out the
/// `duplicated mapping key` failure modes a string-injection approach
/// is prone to, and keeps the on-disk file in sync with the schema as
/// new fields are added.
///
/// Pacquet's `.npmrc` sets `ignore-scripts=true` so no scripts actually
/// run, but pnpm still warns about `ERR_PNPM_IGNORED_BUILDS` for
/// packages whose postinstalls would have fired — the manifest's
/// `allowBuilds: {core-js: false, es5-ext: false, fsevents: false}`
/// silences those specific warnings and keeps pnpm's output clean so
/// hyperfine doesn't see stderr noise.
///
/// Always guarantees `storeDir: ./store-dir` and `cacheDir: ./cache-dir`
/// end up in the destination. Both pnpm and pacquet read these from this
/// file (pacquet since the `.npmrc` parser explicitly ignores
/// `store-dir`); without them, both fall through to the global default
/// store/cache and the benchmark's per-iteration / pre-benchmark cleanup
/// wipes a directory the install never wrote to. That silently
/// invalidates cold/hot-cache semantics and lets state from previous runs
/// leak in (Copilot review on [#296](https://github.com/pnpm/pacquet/pull/296)).
/// `cacheDir` is the resolution-metadata mirror specifically: keeping it
/// local is what lets the cold-cache scenarios force a real cold resolve.
///
/// If a custom fixture's workspace file already declares `storeDir`,
/// trust it — that's the user opting into a different store layout
/// (e.g. shared store across revisions to test a specific scenario).
/// Only inject our default when the key is absent.
///
/// Also mirrors the `.npmrc` settings (`registry`, `autoInstallPeers`,
/// `ignoreScripts`, `lockfile`) into the workspace file as camelCase
/// keys so pnpm 10 picks them up from either source. Per pnpm's reader
/// (`config/reader/src/index.ts:802-808` at pnpm/pnpm@8eb1be4988),
/// non-camelCase keys in `pnpm-workspace.yaml` are silently dropped, so
/// the npmrc spelling can't be reused verbatim. Pacquet's npmrc parser
/// is the only thing that reads `.npmrc` here; pnpm reads both.
pub(super) fn create_pnpm_workspace(
    dst_dir: &Path,
    src_dir: Option<&Path>,
    registry: &str,
    scenario: BenchmarkScenario,
) {
    let dst = dst_dir.join("pnpm-workspace.yaml");
    let src_dir = if scenario.uses_peer_heavy_fixture() { None } else { src_dir };
    let mut manifest = fixture_workspace_manifest(src_dir, &dst)
        .unwrap_or_else(MinimalWorkspaceManifest::default_for_benchmark);
    if manifest.store_dir.is_none() {
        manifest.store_dir = Some("./store-dir".to_string());
    }
    // Force the packument-metadata cache bench-local too, for the same
    // per-iteration-wipe reason as `storeDir`. Left at the global default
    // (`~/.cache/pnpm`), the metadata mirror survives every cold-cache
    // wipe, so a direct install resolves from a warm mirror and never pays
    // the packument-fetch waterfall pnpr is built to offload — "cold
    // cache" would then wipe only the CAS, not the resolution cache.
    if manifest.cache_dir.is_none() {
        manifest.cache_dir = Some("./cache-dir".to_string());
    }
    // Pin `packages: ['.']` when the fixture didn't set one.
    // Without this the fresh-resolve install path's project walker
    // (`find_workspace_projects`) defaults to `[".", "**"]` and recurses
    // into the per-revision `<bench_dir>/pacquet/` clone of pnpm/pnpm,
    // tripping on the intentionally malformed test fixture at
    // `workspace/project-manifest-reader/__fixtures__/invalid-package-json/package.json`.
    if manifest.packages.is_none() {
        manifest.packages = Some(vec![".".to_string()]);
    }
    manifest.registry = Some(registry.to_string());
    manifest.auto_install_peers = Some(true);
    manifest.ignore_scripts = Some(true);
    manifest.lockfile = Some(scenario.lockfile_enabled());
    if scenario.enables_gvs() {
        manifest.enable_global_virtual_store = Some(true);
    }
    let yaml = serde_saphyr::to_string(&manifest).expect("serialize pnpm-workspace.yaml");
    fs::write(dst, yaml).expect("write pnpm-workspace.yaml for the revision");
}
/// The fixture's own `pnpm-workspace.yaml`, when it ships one.
pub(super) fn fixture_workspace_manifest(
    src_dir: Option<&Path>,
    dst: &Path,
) -> Option<MinimalWorkspaceManifest> {
    let src = src_dir?.join("pnpm-workspace.yaml");
    if !src.is_file() {
        return None;
    }
    assert_ne!(src, dst);
    let text = fs::read_to_string(&src).expect("read fixture pnpm-workspace.yaml");
    let parsed: MinimalWorkspaceManifest =
        serde_saphyr::from_str(&text).expect("parse fixture pnpm-workspace.yaml");
    if parsed.store_dir.is_none() {
        eprintln!(
            "warn: fixture's pnpm-workspace.yaml has no top-level `storeDir:` — \
             injecting `storeDir: ./store-dir` so per-revision store isolation works",
        );
    }
    Some(parsed)
}
pub(super) fn create_npmrc(dir: &Path, registry: &str, scenario: BenchmarkScenario) {
    let path = dir.join(".npmrc");
    eprintln!("Creating config file {path:?}...");
    let mut file = File::create(path).expect("create .npmrc");
    writeln!(file, "registry={registry}").unwrap();
    // `store-dir` is read from `pnpm-workspace.yaml` (`storeDir`) by both
    // pnpm and pacquet, not from `.npmrc`. Pacquet's `.npmrc` parser
    // (`crates/npmrc/src/npmrc_auth.rs`) explicitly ignores `store-dir`
    // and a test there pins that behaviour. The static fixture's
    // `storeDir: ./store-dir` resolves to `{bench_dir}/store-dir`
    // under each per-revision CWD, which gives per-revision isolation.
    writeln!(file, "auto-install-peers=true").unwrap();
    writeln!(file, "ignore-scripts=true").unwrap();
    writeln!(file, "{}", scenario.npmrc_lockfile_setting()).unwrap();
}
