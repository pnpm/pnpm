use base64::{Engine, engine::general_purpose};
use flate2::{Compression, write::GzEncoder};
use node_semver::{Range, Version};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256, Sha512};
use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use walkdir::WalkDir;

const PACKAGES_DIR: &str = "pnpr/.fixtures/packages";
const GENERATED_DIR: &str = "pnpr-fixtures";
const COMPLETE_FILE: &str = ".complete";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn ensure_storage() -> &'static Path {
    static STORAGE: OnceLock<PathBuf> = OnceLock::new();
    STORAGE.get_or_init(|| {
        let workspace = workspace_root();
        let packages = workspace.join(PACKAGES_DIR);
        let generated = target_dir(&workspace).join(GENERATED_DIR);
        let fingerprint = fixture_fingerprint(&packages);
        let storage = generated.join("storage").join(&fingerprint);
        ensure_storage_for_fingerprint(&packages, &generated, &storage);
        storage
    })
}

#[must_use]
pub fn packages_dir() -> PathBuf {
    workspace_root().join(PACKAGES_DIR)
}

/// Build verdaccio-shaped storage from the raw package fixtures in `packages`
/// into `out`, replacing any existing contents. Used by the `pnpr-prepare`
/// binary so the JS test harness can serve the moved fixtures; pacquet's own
/// tests use [`ensure_storage`] (process-global, cached) instead.
pub fn build_storage_at(packages: &Path, out: &Path) {
    if out.exists() {
        fs::remove_dir_all(out).expect("clear existing registry fixture storage");
    }
    fs::create_dir_all(out).expect("create registry fixture storage dir");
    build_storage(packages, out);
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
    let temp = generated.join(format!(
        "storage.tmp.{}.{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed),
    ));
    if temp.exists() {
        fs::remove_dir_all(&temp).expect("remove stale temp registry fixture storage");
    }
    build_storage(packages, &temp);
    fs::write(temp.join(COMPLETE_FILE), "").expect("write registry fixture completion marker");
    match fs::rename(&temp, storage) {
        Ok(()) => {}
        Err(_) if storage.join(COMPLETE_FILE).exists() => {
            fs::remove_dir_all(&temp).expect("remove redundant registry fixture storage");
        }
        Err(err) => panic!("publish generated registry fixture storage: {err}"),
    }
}

fn fixture_fingerprint(root: &Path) -> String {
    let mut hasher = Sha256::new();
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

fn build_storage(fixtures_root: &Path, storage_root: &Path) {
    let mut packages: HashMap<String, Package> = HashMap::new();
    for manifest_path in fixture_manifests(fixtures_root) {
        let version = PackageVersion::load(fixtures_root, &manifest_path);
        packages
            .entry(version.name.clone())
            .or_insert_with(|| Package::new(version.name.clone()))
            .versions
            .insert(version.version.clone(), version);
    }
    assert!(!packages.is_empty(), "no registry package fixtures found under {fixtures_root:?}");
    for package in packages.values_mut() {
        package.latest =
            latest_version(package.versions.keys()).expect("package has at least one version");
        package.write(storage_root);
    }
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

    fn write(&self, storage_root: &Path) {
        let package_dir = storage_root.join(&self.name);
        fs::create_dir_all(&package_dir).expect("create test registry package storage dir");
        for version in self.versions.values() {
            fs::write(package_dir.join(&version.tarball_name), &version.tarball)
                .expect("write fixture tarball to test registry storage");
        }
        fs::write(
            package_dir.join("package.json"),
            serde_json::to_vec(&self.packument()).expect("serialize fixture packument"),
        )
        .expect("write fixture packument to test registry storage");
    }

    fn packument(&self) -> Value {
        let versions = self
            .versions
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
    fn load(root: &Path, manifest_path: &Path) -> Self {
        let package_dir = manifest_path.parent().expect("manifest has parent");
        let manifest_text = fs::read_to_string(manifest_path).expect("read fixture package.json");
        let manifest: Value =
            serde_json::from_str(&manifest_text).expect("parse fixture package.json");
        let name = manifest
            .get("name")
            .and_then(Value::as_str)
            .expect("fixture package.json has string name")
            .to_string();
        let version = manifest
            .get("version")
            .and_then(Value::as_str)
            .expect("fixture package.json has string version")
            .to_string();
        let tarball = build_tarball(root, package_dir, &manifest);
        let integrity =
            format!("sha512-{}", general_purpose::STANDARD.encode(Sha512::digest(&tarball)));
        let tarball_name = format!("{}-{version}.tgz", tarball_basename(&name));
        let tarball_url = format!("http://example.test/{name}/-/{tarball_name}");
        let mut packument_manifest = manifest;
        let manifest_object =
            packument_manifest.as_object_mut().expect("fixture package.json is an object");
        manifest_object
            .insert("dist".to_string(), json!({ "tarball": tarball_url, "integrity": integrity }));
        // Verdaccio's abbreviated metadata exposes `bundleDependencies` (no "d"),
        // and that is the key pnpm reads, so mirror `bundledDependencies` onto it
        // when only the longer spelling is present in the fixture manifest.
        if let Some(bundled) = manifest_object.get("bundledDependencies").cloned() {
            manifest_object.entry("bundleDependencies").or_insert(bundled);
        }
        Self { name, version, packument_manifest, tarball_name, tarball }
    }
}

fn fixture_manifests(root: &Path) -> Vec<PathBuf> {
    fixture_files(root)
        .into_iter()
        .map(walkdir::DirEntry::into_path)
        .filter(|path| {
            path.file_name().is_some_and(|name| name == "package.json")
                && is_version_dir(path.parent())
        })
        .collect()
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

fn build_tarball(root: &Path, package_dir: &Path, manifest: &Value) -> Vec<u8> {
    let name = manifest.get("name").and_then(Value::as_str).unwrap_or_default();
    let gzip = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(gzip);
    for entry in fixture_files(package_dir) {
        let relative = entry.path().strip_prefix(package_dir).expect("fixture entry under package");
        let path_in_archive = Path::new("package").join(relative);
        let content = fs::read(entry.path()).expect("read fixture file");
        let mode = file_mode(root, entry.path(), &content).expect("read fixture file mode");
        append_file(&mut tar, &path_in_archive, &content, mode);
    }
    // Files whose names differ only by case cannot coexist in a case-insensitive
    // working tree (the default on macOS and Windows), so they are composed into
    // the archive here instead of being committed as colliding fixture files.
    for (relative, content) in in_memory_files(name) {
        let path_in_archive = Path::new("package").join(relative);
        append_file(&mut tar, &path_in_archive, content.as_bytes(), 0o644);
    }
    // pnpm's `publish` copies the workspace-root LICENSE into every package that
    // doesn't ship its own; registry-mock published these fixtures that way, so
    // reproduce the injected LICENSE here.
    if should_inject_root_license(name) && !package_dir.join("LICENSE").exists() {
        append_file(&mut tar, Path::new("package/LICENSE"), INJECTED_LICENSE.as_bytes(), 0o644);
    }
    // `bundleDependencies` packages publish their resolved dependency tree inside
    // the tarball's `node_modules`. registry-mock produces this with a
    // `prepublishOnly` install; reproduce it here so `node_modules` (gitignored)
    // never has to be committed.
    for (relative, content, mode) in bundled_node_modules(root, manifest) {
        let path_in_archive = Path::new("package").join(relative);
        append_file(&mut tar, &path_in_archive, &content, mode);
    }
    let gzip = tar.into_inner().expect("finish tar archive");
    gzip.finish().expect("finish gzip archive")
}

const INJECTED_LICENSE: &str = include_str!("../../../../LICENSE");

// The bundle-dependency fixtures publish via a `prepublishOnly` install that
// turns each into a self-contained workspace with no root LICENSE to copy, and
// a couple of special fixtures were likewise published without one. Everything
// else receives the injected root LICENSE, matching the registry-mock tarballs.
fn should_inject_root_license(name: &str) -> bool {
    !matches!(
        name,
        "@pnpm.e2e/pkg-with-bundle-dependencies"
            | "@pnpm.e2e/pkg-with-bundle-dependencies-true"
            | "@pnpm.e2e/pkg-with-bundle-dependencies-false"
            | "@pnpm.e2e/pkg-with-bundled-dependencies"
            | "@pnpm.e2e/pkg-with-accidentally-published-catalog-protocol",
    )
}

fn in_memory_files(name: &str) -> &'static [(&'static str, &'static str)] {
    match name {
        "@pnpm.e2e/with-same-file-in-different-cases" => {
            &[("Foo.js", "// Foo.js\n"), ("foo.js", "// foo.js\n")]
        }
        _ => &[],
    }
}

fn bundled_node_modules(root: &Path, manifest: &Value) -> Vec<(PathBuf, Vec<u8>, u32)> {
    let mut files = Vec::new();
    for dep in bundled_dependency_names(manifest) {
        let spec = manifest
            .get("dependencies")
            .and_then(|deps| deps.get(&dep))
            .and_then(Value::as_str)
            .unwrap_or("*");
        let Some(version) = resolve_fixture_version(root, &dep, spec) else { continue };
        let dep_dir = root.join(&dep).join(&version);
        for entry in fixture_files(&dep_dir) {
            let relative = entry
                .path()
                .strip_prefix(&dep_dir)
                .expect("bundled dependency entry under dep dir");
            let archive = Path::new("node_modules").join(&dep).join(relative);
            let content = fs::read(entry.path()).expect("read bundled dependency file");
            let mode =
                file_mode(root, entry.path(), &content).expect("bundled dependency file mode");
            files.push((archive, content, mode));
        }
    }
    files
}

fn bundled_dependency_names(manifest: &Value) -> Vec<String> {
    let bundled =
        manifest.get("bundleDependencies").or_else(|| manifest.get("bundledDependencies"));
    match bundled {
        Some(Value::Bool(true)) => manifest
            .get("dependencies")
            .and_then(Value::as_object)
            .map(|deps| deps.keys().cloned().collect())
            .unwrap_or_default(),
        Some(Value::Array(names)) => {
            names.iter().filter_map(|name| name.as_str().map(String::from)).collect()
        }
        _ => Vec::new(),
    }
}

fn resolve_fixture_version(root: &Path, dep: &str, spec: &str) -> Option<String> {
    let range = Range::parse(spec).ok()?;
    let mut best: Option<(Version, String)> = None;
    for entry in fs::read_dir(root.join(dep)).ok()? {
        let raw = entry.ok()?.file_name().to_string_lossy().into_owned();
        let Ok(version) = Version::parse(&raw) else { continue };
        if range.satisfies(&version) && best.as_ref().is_none_or(|(best, _)| version > *best) {
            best = Some((version, raw));
        }
    }
    best.map(|(_, raw)| raw)
}

fn fixture_files(root: &Path) -> Vec<walkdir::DirEntry> {
    let mut entries: Vec<_> = WalkDir::new(root)
        .into_iter()
        .map(|entry| entry.expect("walk registry package fixtures"))
        .filter(|entry| entry.file_type().is_file())
        .collect();
    entries.sort_by(|left, right| left.path().cmp(right.path()));
    entries
}

fn append_file<Writer: Write>(
    tar: &mut tar::Builder<Writer>,
    path_in_archive: &Path,
    content: &[u8],
    mode: u32,
) {
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(mode);
    header.set_cksum();
    tar.append_data(&mut header, path_in_archive, content).expect("append fixture file");
}

fn file_mode(root: &Path, source: &Path, content: &[u8]) -> io::Result<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(source)?.permissions().mode() & 0o777;
        if mode & 0o111 != 0 {
            return Ok(mode);
        }
    }
    let relative = source.strip_prefix(root).expect("fixture source under root");
    if content.starts_with(b"#!")
        || relative.components().any(|component| component.as_os_str() == "bin")
    {
        return Ok(0o755);
    }
    Ok(0o644)
}

fn tarball_basename(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_string()
}

fn latest_version<'a>(versions: impl Iterator<Item = &'a String>) -> Option<String> {
    versions
        .filter_map(|raw| Version::parse(raw).ok().map(|version| (version, raw.clone())))
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, raw)| raw)
}

#[cfg(test)]
mod tests;
