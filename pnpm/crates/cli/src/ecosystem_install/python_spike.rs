//! Executable architecture experiment, compiled only in CLI unit tests.
//! See `pnpm/plans/PYTHON_SPIKE.md` for the supported fixture subset.

mod tests;
mod wheel;

use super::{EcosystemManifest, EcosystemWorkspaceInventory};
use miette::{IntoDiagnostic, Result, bail};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::{StoreDir, StoreIndex, StoreIndexWriter};
use pnpm_tarball::{ArchiveStoreProjection, IngestZipArchiveToStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
};
use url::Url;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct PythonLock {
    lock_version: String,
    created_by: String,
    packages: Vec<LockedPackage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedPackage {
    name: String,
    version: String,
    wheels: Vec<LockedWheel>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedWheel {
    name: String,
    url: String,
    hashes: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct ProjectFile {
    project: Project,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Project {
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    dynamic: Vec<String>,
    requires_python: Option<String>,
    optional_dependencies: Option<toml::Value>,
}

#[derive(Deserialize)]
struct SimpleIndex {
    files: Vec<IndexFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct IndexFile {
    filename: String,
    url: String,
    hashes: BTreeMap<String, String>,
    #[serde(default)]
    yanked: serde_json::Value,
    requires_python: Option<String>,
}

struct PythonSpike<'a> {
    http_client: &'a ThrottledClient,
    auth_headers: &'a AuthHeaders,
    store_dir: &'static StoreDir,
    store_index_writer: Arc<StoreIndexWriter>,
    index_url: Url,
    offline: bool,
}

struct PythonPlan {
    lock: PythonLock,
    files: BTreeMap<String, PathBuf>,
}

impl PythonSpike<'_> {
    async fn resolve(&self, inventory: &EcosystemWorkspaceInventory) -> Result<PythonPlan> {
        if self.offline {
            bail!("Python spike offline resolution requires a lockfile");
        }
        let mut pending = VecDeque::new();
        for path in inventory.manifests(EcosystemManifest::Python).await? {
            let contents = tokio::fs::read_to_string(path).await.into_diagnostic()?;
            let manifest: ProjectFile = toml::from_str(&contents).into_diagnostic()?;
            if !manifest.project.dynamic.is_empty()
                || manifest.project.requires_python.is_some()
                || manifest.project.optional_dependencies.is_some()
            {
                bail!(
                    "Python spike does not support dynamic metadata, Python constraints or extras",
                );
            }
            pending.extend(manifest.project.dependencies);
        }
        let mut packages = BTreeMap::<String, LockedPackage>::new();
        let mut files = BTreeMap::new();
        while let Some(requirement) = pending.pop_front() {
            let (name, version) = exact_requirement(&requirement)?;
            if let Some(selected) = packages.get(&name) {
                if selected.version != version {
                    bail!("conflicting Python pins for {name}: {} and {version}", selected.version);
                }
                continue;
            }
            let index_url = self.index_url.join(&format!("{name}/")).into_diagnostic()?;
            let response = self
                .http_client
                .get_bytes_with_secure_auth_headers(index_url.as_str(), self.auth_headers)
                .await
                .into_diagnostic()?;
            if !response.status.is_success() {
                bail!("Python index request for {name} returned {}", response.status);
            }
            let index: SimpleIndex = serde_json::from_slice(&response.body).into_diagnostic()?;
            let filename = wheel_name(&name, &version);
            let mut candidates = index.files.into_iter().filter(|file| file.filename == filename);
            let Some(candidate) = candidates.next() else {
                bail!("Python spike requires an exact py3-none-any wheel for {name}=={version}");
            };
            if candidates.next().is_some() {
                bail!("ambiguous Python wheel {filename}");
            }
            if !matches!(candidate.yanked, serde_json::Value::Null | serde_json::Value::Bool(false))
                || candidate.requires_python.is_some()
            {
                bail!("Python spike refuses yanked wheels and Python version constraints");
            }
            let package = LockedPackage {
                name,
                version,
                wheels: vec![LockedWheel {
                    name: filename,
                    url: index_url.join(&candidate.url).into_diagnostic()?.to_string(),
                    hashes: candidate.hashes,
                }],
            };
            let (package_files, dependencies) = self.ingest(&package).await?;
            merge_files(&mut files, package_files)?;
            pending.extend(dependencies);
            packages.insert(package.name.clone(), package);
        }
        Ok(PythonPlan {
            lock: PythonLock {
                lock_version: "1.0".into(),
                created_by: "pnpm-python-spike".into(),
                packages: packages.into_values().collect(),
            },
            files,
        })
    }

    async fn restore(&self, contents: &str) -> Result<PythonPlan> {
        let lock: PythonLock = toml::from_str(contents).into_diagnostic()?;
        if lock.lock_version != "1.0" {
            bail!("unsupported Python lock version {}", lock.lock_version);
        }
        let mut selected = BTreeMap::new();
        for package in &lock.packages {
            if selected.insert(&package.name, &package.version).is_some() {
                bail!("duplicate Python lock package {}", package.name);
            }
        }
        let mut files = BTreeMap::new();
        for package in &lock.packages {
            let (package_files, dependencies) = self.ingest(package).await?;
            for requirement in dependencies {
                let (name, version) = exact_requirement(&requirement)?;
                if selected.get(&name).copied() != Some(&version) {
                    bail!("Python lock does not satisfy {requirement}");
                }
            }
            merge_files(&mut files, package_files)?;
        }
        Ok(PythonPlan { lock, files })
    }

    async fn ingest(
        &self,
        package: &LockedPackage,
    ) -> Result<(BTreeMap<String, PathBuf>, Vec<String>)> {
        let (name, version) = exact_requirement(&format!("{}=={}", package.name, package.version))?;
        let [wheel] = package.wheels.as_slice() else {
            bail!("Python spike requires exactly one wheel per package");
        };
        if name != package.name || wheel.name != wheel_name(&name, &version) {
            bail!("Python lock wheel identity mismatch for {}", package.name);
        }
        let url = Url::parse(&wheel.url).into_diagnostic()?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            bail!("Python spike requires an HTTP(S) wheel URL without credentials");
        }
        let Some(digest) = wheel.hashes.get("sha256") else {
            bail!("Python wheel requires a SHA-256 digest");
        };
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("invalid Python wheel SHA-256 digest");
        }
        let integrity =
            ssri::Integrity::from_hex(digest, ssri::Algorithm::Sha256).into_diagnostic()?;
        let package_id = format!("python:{}", wheel.name);
        let files = IngestZipArchiveToStore {
            http_client: self.http_client,
            store_dir: self.store_dir,
            store_index: StoreIndex::shared_for(self.store_dir, false),
            store_index_writer: Some(Arc::clone(&self.store_index_writer)),
            verify_store_integrity: true,
            strict_store_pkg_content_check: true,
            verified_files_cache: Arc::default(),
            package_integrity: &integrity,
            package_url: &wheel.url,
            package_id: &package_id,
            requester: "python spike",
            prefetched_cas_paths: None,
            retry_opts: RetryOpts { retries: 0, ..Default::default() },
            auth_headers: self.auth_headers,
            archive_prefix: None,
            ignore_file_pattern: None,
            offline: self.offline,
            store_projection: ArchiveStoreProjection::RawArchive,
        }
        .run_without_mem_cache::<SilentReporter>()
        .await
        .into_diagnostic()?;
        let files = files.into_iter().collect();
        let dependencies = wheel::validate(package, &files).await?;
        Ok((files, dependencies))
    }
}

impl PythonPlan {
    fn metadata_paths(&self, root: &Path) -> Vec<PathBuf> {
        std::iter::once(root.join("pylock.toml"))
            .chain(self.files.keys().map(|path| root.join("site-packages").join(path)))
            .collect()
    }

    async fn write(&self, root: &Path) -> Result<()> {
        for (path, source) in &self.files {
            let target = root.join("site-packages").join(path);
            tokio::fs::create_dir_all(target.parent().expect("projection file has parent"))
                .await
                .into_diagnostic()?;
            let bytes = tokio::fs::read(source).await.into_diagnostic()?;
            pnpm_fs::write_atomic(&target, &bytes).into_diagnostic()?;
        }
        pnpm_fs::write_atomic(
            &root.join("pylock.toml"),
            toml::to_string(&self.lock).into_diagnostic()?.as_bytes(),
        )
        .into_diagnostic()
    }
}

fn wheel_name(name: &str, version: &str) -> String {
    format!("{}-{version}-py3-none-any.whl", name.replace('-', "_"))
}

fn exact_requirement(requirement: &str) -> Result<(String, String)> {
    let Some((name, version)) = requirement.split_once("==") else {
        bail!("Python spike requires an exact pin: {requirement}");
    };
    let name = name.trim();
    let version = version.trim();
    if name.is_empty()
        || !name.starts_with(|character: char| character.is_ascii_alphanumeric())
        || !name.ends_with(|character: char| character.is_ascii_alphanumeric())
        || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        || version.is_empty()
        || !version.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
        })
    {
        bail!("unsupported Python spike requirement: {requirement}");
    }
    let name = name
        .split(['-', '_', '.'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_ascii_lowercase();
    Ok((name, version.to_string()))
}

fn merge_files(
    files: &mut BTreeMap<String, PathBuf>,
    additional: BTreeMap<String, PathBuf>,
) -> Result<()> {
    for (path, source) in additional {
        if files.insert(path.clone(), source).is_some() {
            bail!("Python wheels install the same path: {path}");
        }
    }
    Ok(())
}
