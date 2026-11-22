use super::{
    BTreeSet,
    IntoDiagnostic,
    Path,
    PathBuf,
    Result,
    StreamExt,
    TryStreamExt,
    WORKSPACE_INSTALL_CONCURRENCY,
    canonical_cargo_path,
    run_cargo_metadata,
    stream,
};
use miette::WrapErr;

/// `document`, a `cargo metadata --no-deps` document, with the packages of the
/// path dependencies outside the workspace appended, as Cargo locks those too.
/// A package outside the workspace is followed through its own path
/// dependencies, but not its dev-dependencies, which Cargo does not lock.
///
/// `cargo` is the workspace's own: a path dependency outside the checkout has
/// no toolchain pnpm provisioned for it.
pub(super) async fn with_path_packages(document: &str, cargo: &Path) -> Result<String> {
    let mut document = serde_json::from_str::<serde_json::Value>(document)
        .into_diagnostic()
        .wrap_err("parse cargo metadata")?;
    let packages = document
        .get_mut("packages")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| miette::miette!("cargo metadata has no `packages` array"))?;
    let mut known = packages
        .iter()
        .filter_map(package_manifest_path)
        .map(|manifest| canonical_cargo_path(&manifest))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut pending = packages
        .iter()
        .flat_map(|package| path_dependency_manifests(package, DevDependencies::Followed))
        .collect::<Vec<_>>();
    while !pending.is_empty() {
        let mut wave = Vec::new();
        for manifest in std::mem::take(&mut pending) {
            let manifest = canonical_cargo_path(&manifest)?;
            if known.insert(manifest.clone()) {
                wave.push(manifest);
            }
        }
        let found = stream::iter(wave)
            .map(|manifest| async move { path_package(&manifest, cargo).await })
            .buffer_unordered(WORKSPACE_INSTALL_CONCURRENCY)
            .try_collect::<Vec<_>>()
            .await?;
        for package in found {
            pending.extend(path_dependency_manifests(&package, DevDependencies::Skipped));
            packages.push(package);
        }
    }
    packages.iter_mut().for_each(canonicalize_paths);
    serde_json::to_string(&document).into_diagnostic().wrap_err("serialize cargo metadata")
}

/// The `cargo metadata` entry of the package `manifest` declares. The
/// manifest may belong to a workspace of its own, which Cargo lists whole.
async fn path_package(manifest: &Path, cargo: &Path) -> Result<serde_json::Value> {
    let document = run_cargo_metadata(cargo.to_path_buf(), manifest).await?;
    let document = serde_json::from_str::<serde_json::Value>(&document)
        .into_diagnostic()
        .wrap_err("parse cargo metadata")?;
    document
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|package| {
            package_manifest_path(package)
                .is_some_and(|path| canonical_cargo_path(&path).is_ok_and(|path| path == manifest))
        })
        .cloned()
        .ok_or_else(|| {
            let manifest = manifest.display();
            miette::miette!("cargo metadata lists no package for {manifest}")
        })
}

/// Spell a package's `manifest_path` and its dependencies' `path`s the same
/// way, so the resolver finds the package a path dependency points to even
/// when the two were reached through different symlinks. A path that does not
/// resolve is left as Cargo wrote it.
fn canonicalize_paths(package: &mut serde_json::Value) {
    canonicalize_field(package, "manifest_path");
    let dependencies = package.get_mut("dependencies").and_then(serde_json::Value::as_array_mut);
    for dependency in dependencies.into_iter().flatten() {
        canonicalize_field(dependency, "path");
    }
}

fn canonicalize_field(object: &mut serde_json::Value, key: &str) {
    let Some(field) = object.get_mut(key) else { return };
    let canonical = field
        .as_str()
        .map(dunce::canonicalize)
        .and_then(Result::ok)
        .and_then(|path| path.to_str().map(str::to_string));
    if let Some(canonical) = canonical {
        *field = canonical.into();
    }
}

fn package_manifest_path(package: &serde_json::Value) -> Option<PathBuf> {
    package
        .get("manifest_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
}

/// Whether a package's path dev-dependencies are followed: Cargo locks them
/// for a workspace member only.
#[derive(Clone, Copy)]
enum DevDependencies {
    Followed,
    Skipped,
}

fn path_dependency_manifests(
    package: &serde_json::Value,
    dev_dependencies: DevDependencies,
) -> impl Iterator<Item = PathBuf> + '_ {
    package
        .get("dependencies")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(move |dependency| match dev_dependencies {
            DevDependencies::Followed => true,
            DevDependencies::Skipped => {
                dependency.get("kind").and_then(serde_json::Value::as_str) != Some("dev")
            }
        })
        .filter_map(|dependency| dependency.get("path"))
        .filter_map(serde_json::Value::as_str)
        .map(Path::new)
        .map(|path| path.join("Cargo.toml"))
}
