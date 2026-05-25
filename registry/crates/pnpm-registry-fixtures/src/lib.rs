use base64::{Engine, engine::general_purpose};
use flate2::{Compression, write::GzEncoder};
use node_semver::Version;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256, Sha512};
use std::{
    collections::{BTreeMap, HashMap},
    env, fs, io,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use walkdir::WalkDir;

const PACKAGES_DIR: &str = "registry/.fixtures/packages";
const GENERATED_DIR: &str = "pnpm-registry-fixtures";
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

pub fn packages_dir() -> PathBuf {
    workspace_root().join(PACKAGES_DIR)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("registry fixture crate lives under registry/crates")
        .to_path_buf()
}

fn target_dir(workspace: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| workspace.join("target"))
}

fn ensure_storage_for_fingerprint(packages: &Path, generated: &Path, storage: &Path) {
    if storage.join(COMPLETE_FILE).exists() {
        return;
    }
    fs::create_dir_all(generated).expect("create generated registry fixture dir");
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
        times.insert("created".to_string(), json!("2020-01-01T00:00:00.000Z"));
        times.insert("modified".to_string(), json!("2020-01-01T00:00:00.000Z"));
        for version in self.versions.keys() {
            times.insert(version.clone(), json!("2020-01-01T00:00:00.000Z"));
        }
        Value::Object(times)
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
        let tarball = build_tarball(root, package_dir);
        let integrity =
            format!("sha512-{}", general_purpose::STANDARD.encode(Sha512::digest(&tarball)));
        let tarball_name = format!("{}-{version}.tgz", tarball_basename(&name));
        let tarball_url = format!("http://example.test/{name}/-/{tarball_name}");
        let mut packument_manifest = manifest;
        packument_manifest
            .as_object_mut()
            .expect("fixture package.json is an object")
            .insert("dist".to_string(), json!({ "tarball": tarball_url, "integrity": integrity }));
        Self { name, version, packument_manifest, tarball_name, tarball }
    }
}

fn fixture_manifests(root: &Path) -> Vec<PathBuf> {
    fixture_files(root)
        .into_iter()
        .map(walkdir::DirEntry::into_path)
        .filter(|path| path.file_name().is_some_and(|name| name == "package.json"))
        .collect()
}

fn build_tarball(root: &Path, package_dir: &Path) -> Vec<u8> {
    let gzip = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(gzip);
    for entry in fixture_files(package_dir) {
        let relative = entry.path().strip_prefix(package_dir).expect("fixture entry under package");
        let path_in_archive = Path::new("package").join(relative);
        append_file(&mut tar, root, entry.path(), &path_in_archive);
    }
    let gzip = tar.into_inner().expect("finish tar archive");
    gzip.finish().expect("finish gzip archive")
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
    root: &Path,
    source: &Path,
    path_in_archive: &Path,
) {
    let content = fs::read(source).expect("read fixture file");
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(file_mode(root, source, &content).expect("read fixture file mode"));
    header.set_cksum();
    tar.append_data(&mut header, path_in_archive, content.as_slice()).expect("append fixture file");
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
mod tests {
    use super::{ensure_storage, latest_version, packages_dir};

    #[test]
    fn latest_version_uses_semver_prerelease_order() {
        let versions =
            ["1.0.0-beta.2".to_string(), "1.0.0-beta.10".to_string(), "1.0.0".to_string()];
        assert_eq!(latest_version(versions.iter()), Some("1.0.0".to_string()));
    }

    #[test]
    fn ensure_storage_generates_packuments_and_tarballs() {
        let storage = ensure_storage();
        assert!(packages_dir().join("is-positive/1.0.0/package.json").exists());
        assert!(storage.join("is-positive/package.json").exists());
        assert!(storage.join("is-positive/is-positive-1.0.0.tgz").exists());
    }
}
