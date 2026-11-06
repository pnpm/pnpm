pub mod presets;
pub use metadata::{
    deprecate_version, read_hosted_document, read_hosted_file, read_packument, remove_version,
    set_dist_tag, tarball_path, write_hosted_document, write_hosted_file, write_packuments,
    write_packuments_into,
};
pub use presets::{
    MockConfigPresets, REGISTRY_MOCK_LOCAL_PATTERNS, REGISTRY_MOCK_UPSTREAM_TIMEOUT, TAR_REGISTRY,
    proxy_config, registry_mock_graph, registry_mock_rules, static_serve_config, with_tar_registry,
};

mod metadata;
mod tarball;
use tarball::{build_tarball, fixture_files};

use base64::{Engine, engine::general_purpose};
use flate2::{Compression, write::GzEncoder};
use node_semver::{Range, Version};
use pnpr_registry::Ecosystem;
use pnpr_storage::{
    BlobId,
    layout::{local_archive_path, write_local_layout_marker},
    metadata::{ArtifactBlob, BlobRef},
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256, Sha512};
use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
};
use walkdir::WalkDir;

pub const MOCK_CONFIG_PATH: &str = "pnpr/.fixtures/config.yaml";
pub const MOCK_CONFIG_YAML: &str = include_str!("../../../.fixtures/config.yaml");

#[must_use]
pub fn mock_config_path() -> PathBuf {
    workspace_root().join(MOCK_CONFIG_PATH)
}

const PACKAGES_DIR: &str = "pnpr/.fixtures/packages";
const GENERATED_DIR: &str = "pnpr-fixtures";
const COMPLETE_FILE: &str = ".complete";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[must_use]
pub fn ensure_storage() -> &'static Path {
    static STORAGE: LazyLock<PathBuf> = LazyLock::new(|| {
        let workspace = workspace_root();
        let packages = workspace.join(PACKAGES_DIR);
        let generated = target_dir(&workspace).join(GENERATED_DIR);
        let fingerprint = fixture_fingerprint(&packages);
        let storage = generated.join("storage").join(&fingerprint);
        ensure_storage_for_fingerprint(&packages, &generated, &storage);
        storage
    });
    STORAGE.as_path()
}

/// The metadata database of the fixture storage at `storage`, the one pnpr
/// reads by default.
#[must_use]
pub fn metadata_path(storage: &Path) -> PathBuf {
    pnpr_config::default_metadata_path(storage)
}

#[must_use]
pub fn packages_dir() -> PathBuf {
    workspace_root().join(PACKAGES_DIR)
}

/// Build registry storage from the raw package fixtures in `packages` into
/// `out`, replacing any existing contents: the tarballs as hosted blobs and
/// the packuments in the `SQLite` metadata database pnpr reads by default.
/// Used by the `pnpr-prepare` binary so the JS test harness can serve the
/// moved fixtures; pacquet's own tests use [`ensure_storage`]
/// (process-global, cached) instead.
pub fn build_storage_at(packages: &Path, out: &Path) {
    build_storage_at_with_substitutions(packages, out, &[]);
}

/// Build fixture storage after replacing exact strings in every top-level
/// fixture manifest. Tests use this to point committed registry fixtures at a
/// per-run `git+file://` repository without making the default fixture set
/// depend on a local path.
pub fn build_storage_at_with_substitutions(
    packages: &Path,
    out: &Path,
    substitutions: &[(&str, &str)],
) {
    build_storage_at_with_edits(packages, out, substitutions, &[]);
}

/// A field a fixture version's registry metadata leaves out while its
/// tarball keeps it, so the registry serves metadata that disagrees with the
/// published `package.json`.
#[derive(Debug, Clone, Copy)]
pub struct RemovedVersionField<'name> {
    pub package: &'name str,
    pub version: &'name str,
    pub field: &'name str,
}

/// [`build_storage_at_with_substitutions`], with `removed_fields` left out
/// of the registry metadata. The store never changes a version it accepted,
/// so a field is removed as the storage is built, not afterwards.
pub fn build_storage_at_with_edits(
    packages: &Path,
    out: &Path,
    substitutions: &[(&str, &str)],
    removed_fields: &[RemovedVersionField<'_>],
) {
    remove_tree(out, "clear existing registry fixture storage");
    fs::create_dir_all(out).expect("create registry fixture storage dir");
    build_storage(packages, out, substitutions, removed_fields);
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("registry fixture crate lives under pnpr/crates")
        .to_path_buf()
}

fn target_dir(workspace: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR").map_or_else(|| workspace.join("target"), PathBuf::from)
}

fn ensure_storage_for_fingerprint(packages: &Path, generated: &Path, storage: &Path) {
    if storage.join(COMPLETE_FILE).exists() {
        return;
    }
    fs::create_dir_all(storage.parent().expect("registry fixture storage has parent"))
        .expect("create generated registry fixture storage dir");
    let temp = generated.join(scratch_name("storage.tmp"));
    remove_tree(&temp, "remove stale temp registry fixture storage");
    build_storage(packages, &temp, &[], &[]);
    fs::write(temp.join(COMPLETE_FILE), "").expect("write registry fixture completion marker");
    publish_storage(generated, &temp, storage);
}

/// Move a freshly built `temp` tree into place at `storage`.
///
/// Publishers race: the loser's rename fails because the winner's tree is
/// already there, which is harmless — the completion marker proves the
/// content arrived. A tree *without* the marker is a different matter. It
/// is a half-finished publish, or a build cache restored without the
/// marker, and no reader may touch it; left alone it wedges every later
/// run with `Directory not empty`.
fn publish_storage(generated: &Path, temp: &Path, storage: &Path) {
    if try_publish_storage(temp, storage).is_ok() {
        return;
    }
    discard_unusable_storage(generated, storage);
    try_publish_storage(temp, storage).expect("publish generated registry fixture storage");
}

/// Clear an unusable tree out of `storage` so a publish can land there.
///
/// Claiming the tree with a rename is what makes this safe against other
/// publishers: exactly one of them can move a given directory, so the
/// others find the path already free (or already republished) rather than
/// deleting each other's work.
///
/// The claim is taken before the marker can be inspected, though, and a
/// competing publisher may have completed the tree in between. Deleting it
/// then would pull content out from under every reader holding the path,
/// so the marker is re-checked *after* the claim and a completed tree is
/// put straight back.
fn discard_unusable_storage(generated: &Path, storage: &Path) {
    let claimed = generated.join(scratch_name("storage.stale"));
    if fs::rename(storage, &claimed).is_err() {
        return;
    }
    if claimed.join(COMPLETE_FILE).exists() {
        restore_claimed_storage(&claimed, storage);
        return;
    }
    remove_tree(&claimed, "remove unusable registry fixture storage");
}

/// Put a claimed tree back after it turned out to be complete.
///
/// The claim leaves `storage` free, so a publisher can land its own tree
/// there before the restore runs. Its content is the same — the path is
/// addressed by a fingerprint of the fixtures — so a claim that can no
/// longer go back is redundant rather than lost, and gets dropped.
fn restore_claimed_storage(claimed: &Path, storage: &Path) {
    if fs::rename(claimed, storage).is_err() {
        remove_tree(claimed, "remove redundant registry fixture storage");
    }
}

/// Remove `path` and its subtree, treating an absent path as removed.
fn remove_tree(path: &Path, what: &str) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => panic!("{what} at {}: {error}", path.display()),
    }
}

fn try_publish_storage(temp: &Path, storage: &Path) -> io::Result<()> {
    match fs::rename(temp, storage) {
        Ok(()) => Ok(()),
        Err(_) if storage.join(COMPLETE_FILE).exists() => {
            remove_tree(temp, "remove redundant registry fixture storage");
            Ok(())
        }
        Err(err) => Err(err),
    }
}

/// A scratch directory name no other publisher — in this process or any
/// concurrent one — can collide with.
fn scratch_name(prefix: &str) -> String {
    format!("{prefix}.{}.{}", std::process::id(), TEMP_COUNTER.fetch_add(1, Ordering::Relaxed))
}

fn fixture_fingerprint(root: &Path) -> String {
    let mut hasher = Sha256::new();
    // A storage tree built in another layout, or with metadata in another
    // schema, must not be reused.
    hasher.update(pnpr_storage::layout::LAYOUT_VERSION.to_le_bytes());
    hasher.update(pnpr_storage::metadata::SqlMetadata::SCHEMA_VERSION.as_bytes());
    hasher.update(pnpr_storage::metadata::SqlMetadata::SCHEMA_TABLES.as_bytes());
    hasher.update([0]);
    hasher.update(
        local_archive_path(Path::new(""), "", Ecosystem::Npm, "sha512-00.1")
            .to_string_lossy()
            .as_bytes(),
    );
    for entry in fixture_files(root) {
        let path = entry.path();
        let relative = path.strip_prefix(root).expect("fixture entry under root");
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(fs::read(path).expect("read registry fixture for fingerprint"));
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn build_storage(
    fixtures_root: &Path,
    storage_root: &Path,
    substitutions: &[(&str, &str)],
    removed_fields: &[RemovedVersionField<'_>],
) {
    let mut packages: HashMap<String, Package> = HashMap::new();
    for manifest_path in fixture_manifests(fixtures_root) {
        let version = PackageVersion::load(fixtures_root, &manifest_path, substitutions);
        packages
            .entry(version.name.clone())
            .or_insert_with(|| Package::new(version.name.clone()))
            .versions
            .insert(version.version.clone(), version);
    }
    assert!(!packages.is_empty(), "no registry package fixtures found under {fixtures_root:?}");
    let mut packuments = Vec::with_capacity(packages.len());
    let mut archives = Vec::new();
    for package in packages.values_mut() {
        package.latest =
            latest_version(package.versions.keys()).expect("package has at least one version");
        archives.extend(package.write_blobs(storage_root));
        packuments.push((package.name.clone(), package.packument()));
    }
    for removed in removed_fields {
        remove_version_field(&mut packuments, removed);
    }
    write_local_layout_marker(storage_root).expect("write registry fixture layout marker");
    metadata::write_packuments_with_archives(storage_root, packuments, archives);
}

fn remove_version_field(packuments: &mut [(String, Value)], removed: &RemovedVersionField<'_>) {
    let RemovedVersionField { package, version, field } = *removed;
    let (_, packument) = packuments
        .iter_mut()
        .find(|(name, _)| name == package)
        .unwrap_or_else(|| panic!("no fixture package {package}"));
    let removed = packument
        .get_mut("versions")
        .and_then(|versions| versions.get_mut(version))
        .and_then(Value::as_object_mut)
        .unwrap_or_else(|| panic!("{package} has no fixture version {version}"))
        .remove(field);
    assert!(removed.is_some(), "{package}@{version} has no {field} to remove");
}

struct Package {
    name: String,
    latest: String,
    versions: BTreeMap<String, PackageVersion>,
}

impl Package {
    fn new(name: String) -> Self {
        Self { name, latest: String::new(), versions: BTreeMap::new() }
    }

    /// Store every version's tarball as the first incarnation of its bytes,
    /// and return the mapping of each to them, which the metadata commits.
    fn write_blobs(&self, storage_root: &Path) -> Vec<ArtifactBlob> {
        let mut archives = Vec::with_capacity(self.versions.len());
        let name = pnpr_package_name::CanonicalPackageName::parse(&self.name, Ecosystem::Npm)
            .unwrap_or_else(|error| panic!("package name {:?}: {error}", self.name));
        for version in self.versions.values() {
            let blob =
                BlobRef { id: BlobId::of_bytes(Ecosystem::Npm, &version.tarball), incarnation: 1 };
            let path = local_archive_path(storage_root, "", Ecosystem::Npm, &blob.key());
            fs::create_dir_all(path.parent().expect("an archive has a directory"))
                .expect("create test registry archive dir");
            fs::write(&path, &version.tarball)
                .expect("write fixture tarball to test registry storage");
            archives.push(ArtifactBlob {
                ecosystem: Ecosystem::Npm,
                name: name.as_str().to_string(),
                version: version.version.clone(),
                artifact: version.tarball_name.clone(),
                blob,
                size: version.tarball.len() as u64,
            });
        }
        archives
    }

    fn packument(&self) -> Value {
        let versions = self.versions
            .iter()
            .map(|(version, package)| (version.clone(), package.packument_manifest.clone()))
            .collect();
        json!({
            "name": self.name,
            "dist-tags": { "latest": self.latest },
            "versions": Value::Object(versions),
            "time": self.times(),
        })
    }

    fn times(&self) -> Value {
        let mut times = Map::new();
        times.insert("created".to_string(), json!(DEFAULT_PUBLISH_TIME));
        times.insert("modified".to_string(), json!(DEFAULT_PUBLISH_TIME));
        for version in self.versions.keys() {
            times.insert(version.clone(), json!(version_publish_time(&self.name, version)));
        }
        Value::Object(times)
    }
}

// Most fixtures share one old timestamp so `minimumReleaseAge` checks treat them
// as long-published. Packages exercised by time-based resolution tests carry
// distinct per-version times that encode their relative publish order.
const DEFAULT_PUBLISH_TIME: &str = "2022-01-01T00:00:00.000Z";

fn version_publish_time(name: &str, version: &str) -> &'static str {
    match (name, version) {
        ("@pnpm.e2e/bravo", "1.0.0") => "2022-04-01T20:17:46.770Z",
        ("@pnpm.e2e/romeo", "1.0.0") => "2022-01-01T20:17:46.770Z",
        ("@pnpm.e2e/bravo-dep", "1.0.0") => "2022-02-01T20:17:46.770Z",
        ("@pnpm.e2e/bravo-dep", "1.0.1") => "2022-02-22T20:17:46.770Z",
        ("@pnpm.e2e/bravo-dep", "1.1.0") => "2022-05-01T20:17:46.770Z",
        ("@pnpm.e2e/romeo-dep", "1.0.0") => "2022-03-01T20:17:46.770Z",
        ("@pnpm.e2e/romeo-dep", "1.1.0") => "2022-07-01T20:17:46.770Z",
        _ => DEFAULT_PUBLISH_TIME,
    }
}

struct PackageVersion {
    name: String,
    version: String,
    packument_manifest: Value,
    tarball_name: String,
    tarball: Vec<u8>,
}

impl PackageVersion {
    fn load(root: &Path, manifest_path: &Path, substitutions: &[(&str, &str)]) -> Self {
        let package_dir = manifest_path.parent().expect("manifest has parent");
        let manifest_text = substituted_manifest_text(manifest_path, substitutions);
        let manifest: Value =
            serde_json::from_str(&manifest_text).expect("parse fixture package.json");
        let name = manifest_string(&manifest, "name");
        let version = manifest_string(&manifest, "version");
        let tarball = build_tarball(root, package_dir, &manifest, &manifest_text);
        let integrity =
            format!("sha512-{}", general_purpose::STANDARD.encode(Sha512::digest(&tarball)));
        let tarball_name = format!("{}-{version}.tgz", tarball_basename(&name));
        let tarball_url = format!("http://example.test/{name}/-/{tarball_name}");
        let packument_manifest = with_dist(manifest, &tarball_url, &integrity);
        Self { name, version, packument_manifest, tarball_name, tarball }
    }
}

fn fixture_manifests(root: &Path) -> Vec<PathBuf> {
    fixture_files(root)
        .into_iter()
        .map(walkdir::DirEntry::into_path)
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name == "package.json")
                && is_version_dir(path.parent())
        })
        .collect()
}

fn substituted_manifest_text(manifest_path: &Path, substitutions: &[(&str, &str)]) -> String {
    substitutions
        .iter()
        .fold(
            fs::read_to_string(manifest_path).expect("read fixture package.json"),
            |manifest, (from, to)| manifest.replace(from, to),
        )
}

fn manifest_string(manifest: &Value, key: &str) -> String {
    manifest
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("fixture package.json has string {key}"))
        .to_string()
}

/// The manifest as a packument version entry: with its `dist` block, and
/// with `bundleDependencies` mirrored from `bundledDependencies`.
fn with_dist(mut packument_manifest: Value, tarball_url: &str, integrity: &str) -> Value {
    let manifest_object =
        packument_manifest.as_object_mut().expect("fixture package.json is an object");
    manifest_object.insert(
        "dist".to_string(),
        json!({ "tarball": tarball_url, "integrity": integrity }),
    );
    // Verdaccio's abbreviated metadata exposes `bundleDependencies` (no "d"),
    // and that is the key pnpm reads, so mirror `bundledDependencies` onto it
    // when only the longer spelling is present in the fixture manifest.
    if let Some(bundled) = manifest_object.get("bundledDependencies").cloned() {
        manifest_object.entry("bundleDependencies").or_insert(bundled);
    }
    packument_manifest
}

// A `package.json` is a package manifest only when it sits directly inside a
// `<version>` directory. Nested manifests (bundled `node_modules`, file
// dependencies like `has-local-dep/local-dep`) are package contents, not
// separate packages.
fn is_version_dir(dir: Option<&Path>) -> bool {
    dir.and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .is_some_and(|name| Version::parse(name).is_ok())
}

fn tarball_basename(name: &str) -> String {
    name.rsplit('/')
        .next()
        .unwrap_or(name)
        .to_string()
}

fn latest_version<'a>(versions: impl Iterator<Item = &'a String>) -> Option<String> {
    versions
        .filter_map(|raw| {
            Version::parse(raw)
                .ok()
                .map(|version| (version, raw.clone()))
        })
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, raw)| raw)
}

#[cfg(test)]
mod tests;
