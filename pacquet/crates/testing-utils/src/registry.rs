use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use base64::{Engine, engine::general_purpose};
use bytes::Bytes;
use flate2::{Compression, write::GzEncoder};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha512};
use std::{
    collections::{BTreeMap, HashMap},
    fs, io,
    io::Write,
    net::{Ipv4Addr, TcpListener},
    path::{Path as FsPath, PathBuf},
    sync::{Arc, OnceLock},
    thread,
};
use walkdir::WalkDir;

#[derive(Debug)]
#[must_use]
pub struct TestRegistry {
    instance: &'static TestRegistryInstance,
}

impl TestRegistry {
    pub fn start() -> Self {
        Self { instance: TestRegistryInstance::get() }
    }

    pub fn url(&self) -> String {
        self.instance.url.clone()
    }
}

#[derive(Debug)]
struct TestRegistryInstance {
    url: String,
}

impl TestRegistryInstance {
    fn get() -> &'static Self {
        static INSTANCE: OnceLock<TestRegistryInstance> = OnceLock::new();
        INSTANCE.get_or_init(Self::start)
    }

    fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind test registry to an unused localhost port");
        listener.set_nonblocking(true).expect("set test registry listener to nonblocking");
        let listen = listener.local_addr().expect("read test registry listener address");
        let url = format!("http://{listen}/");
        let registry = Arc::new(FixtureRegistry::load(fixtures_dir(), url.trim_end_matches('/')));
        thread::Builder::new()
            .name("pacquet-test-registry".to_string())
            .spawn(move || run_registry(listener, registry))
            .expect("spawn test registry thread");

        Self { url }
    }
}

fn run_registry(listener: TcpListener, registry: Arc<FixtureRegistry>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create test registry runtime");

    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener).expect("create tokio listener");
        let app = Router::new().route("/{*path}", get(route)).with_state(registry);
        axum::serve(listener, app).await.expect("serve test registry");
    });
}

async fn route(State(registry): State<Arc<FixtureRegistry>>, Path(path): Path<String>) -> Response {
    match RequestPath::parse(&path) {
        RequestPath::Packument(name) => registry.packument(&name),
        RequestPath::Latest(name) => registry.latest(&name),
        RequestPath::Tarball { name, version } => registry.tarball(&name, &version),
        RequestPath::Unknown => StatusCode::NOT_FOUND.into_response(),
    }
}

enum RequestPath {
    Packument(String),
    Latest(String),
    Tarball { name: String, version: String },
    Unknown,
}

impl RequestPath {
    fn parse(path: &str) -> Self {
        let path = path.replace("%2f", "/").replace("%2F", "/");
        if let Some((name, filename)) = path.split_once("/-/") {
            let filename_without_extension = filename.strip_suffix(".tgz").unwrap_or(filename);
            let Some(version) = filename_without_extension
                .strip_prefix(&tarball_name(name))
                .and_then(|rest| rest.strip_prefix('-'))
                .map(str::to_string)
            else {
                return Self::Unknown;
            };
            if version.is_empty() {
                return Self::Unknown;
            }
            return Self::Tarball { name: name.to_string(), version };
        }
        if let Some(name) = path.strip_suffix("/latest") {
            return Self::Latest(name.to_string());
        }
        Self::Packument(path)
    }
}

struct FixtureRegistry {
    packages: HashMap<String, Package>,
}

impl FixtureRegistry {
    fn load(root: &FsPath, public_url: &str) -> Self {
        let mut packages: HashMap<String, Package> = HashMap::new();
        for manifest_path in fixture_manifests(root) {
            let version = PackageVersion::load(root, &manifest_path, public_url);
            packages
                .entry(version.name.clone())
                .or_insert_with(|| Package::new(version.name.clone()))
                .versions
                .insert(version.version.clone(), version);
        }
        assert!(!packages.is_empty(), "no registry package fixtures found under {root:?}");
        for package in packages.values_mut() {
            package.latest = package
                .versions
                .keys()
                .max_by(|left, right| compare_versions(left, right))
                .expect("package has at least one version")
                .clone();
        }
        Self { packages }
    }

    fn packument(&self, name: &str) -> Response {
        let Some(package) = self.packages.get(name) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        axum::Json(package.packument()).into_response()
    }

    fn latest(&self, name: &str) -> Response {
        let Some(package) = self.packages.get(name) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let Some(version) = package.versions.get(&package.latest) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        axum::Json(&version.packument_manifest).into_response()
    }

    fn tarball(&self, name: &str, version: &str) -> Response {
        let Some(package) = self.packages.get(name) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let Some(version) = package.versions.get(version) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        Response::builder()
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from(version.tarball.clone()))
            .expect("build tarball response")
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
    tarball: Bytes,
}

impl PackageVersion {
    fn load(root: &FsPath, manifest_path: &FsPath, public_url: &str) -> Self {
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
        let tarball_url = format!("{public_url}/{name}/-/{}-{version}.tgz", tarball_name(&name));
        let mut packument_manifest = manifest;
        packument_manifest
            .as_object_mut()
            .expect("fixture package.json is an object")
            .insert("dist".to_string(), json!({ "tarball": tarball_url, "integrity": integrity }));
        Self { name, version, packument_manifest, tarball }
    }
}

fn fixture_manifests(root: &FsPath) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .map(|entry| entry.expect("walk registry package fixtures"))
        .filter(|entry| entry.file_type().is_file() && entry.file_name() == "package.json")
        .map(walkdir::DirEntry::into_path)
        .collect()
}

fn build_tarball(root: &FsPath, package_dir: &FsPath) -> Bytes {
    let gzip = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(gzip);
    for entry in fixture_files(package_dir) {
        let relative = entry.path().strip_prefix(package_dir).expect("fixture entry under package");
        let path_in_archive = FsPath::new("package").join(relative);
        append_file(&mut tar, root, entry.path(), &path_in_archive);
    }
    let gzip = tar.into_inner().expect("finish tar archive");
    Bytes::from(gzip.finish().expect("finish gzip archive"))
}

fn fixture_files(package_dir: &FsPath) -> Vec<walkdir::DirEntry> {
    let mut entries: Vec<_> = WalkDir::new(package_dir)
        .into_iter()
        .map(|entry| entry.expect("walk fixture package"))
        .filter(|entry| entry.file_type().is_file())
        .collect();
    entries.sort_by(|left, right| left.path().cmp(right.path()));
    entries
}

fn append_file<Writer: Write>(
    tar: &mut tar::Builder<Writer>,
    root: &FsPath,
    source: &FsPath,
    path_in_archive: &FsPath,
) {
    let content = fs::read(source).expect("read fixture file");
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(file_mode(root, source, &content).expect("read fixture file mode"));
    header.set_cksum();
    tar.append_data(&mut header, path_in_archive, content.as_slice()).expect("append fixture file");
}

fn file_mode(root: &FsPath, source: &FsPath, content: &[u8]) -> io::Result<u32> {
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

fn fixtures_dir() -> &'static FsPath {
    FsPath::new(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/registry").leak()
}

fn tarball_name(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_string()
}

fn compare_versions(left: &String, right: &String) -> std::cmp::Ordering {
    parse_version(left).cmp(&parse_version(right)).then_with(|| left.cmp(right))
}

fn parse_version(version: &str) -> Vec<u64> {
    version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::RequestPath;

    #[test]
    fn tarball_path_parses_prerelease_version() {
        match RequestPath::parse("@scope/pkg/-/pkg-1.0.0-beta.1.tgz") {
            RequestPath::Tarball { name, version } => {
                assert_eq!(name, "@scope/pkg");
                assert_eq!(version, "1.0.0-beta.1");
            }
            _ => panic!("expected tarball path"),
        }
    }

    #[test]
    fn tarball_path_rejects_mismatched_filename_prefix() {
        assert!(
            matches!(
                RequestPath::parse("@scope/pkg/-/other-1.0.0.tgz"),
                RequestPath::Unknown,
            ),
        );
    }
}
