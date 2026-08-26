use crate::package_map::{PackageMapOptions, lockfile_to_package_map};
use derive_more::{Display, Error};
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use std::path::{Path, PathBuf};

pub const PNP_FILENAME: &str = ".pnp.cjs";
const PNP_LOADER_TEMPLATE: &str = include_str!("pnp_loader.cjs.inc");
const PACKAGE_REGISTRY_PLACEHOLDER: &str = "__PNPM_PACKAGE_REGISTRY__";
const MODULES_DIR_PLACEHOLDER: &str = "__PNPM_MODULES_DIR__";

#[derive(Debug, Display, Error)]
pub enum WritePnpFileError {
    #[display("failed to serialize the PnP package registry: {_0}")]
    Serialize(#[error(source)] serde_json::Error),
    #[display("failed to write .pnp.cjs: {_0}")]
    Write(#[error(source)] pnpm_fs::EnsureFileError),
}

pub fn write_pnp_file(
    lockfile: &Lockfile,
    lockfile_dir: &Path,
    config: &pnpm_config::Config,
    layout: &crate::VirtualStoreLayout,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> Result<(), WritePnpFileError> {
    let package_map = lockfile_to_package_map(
        lockfile,
        &PackageMapOptions {
            lockfile_dir,
            modules_dir: &config.modules_dir,
            package_map_type: pnpm_config::NodePackageMapType::Standard,
            layout,
            project_manifests,
        },
    );
    let registry = serde_json::to_string(&package_map).map_err(WritePnpFileError::Serialize)?;
    let modules_dir = pathdiff::diff_paths(&config.modules_dir, lockfile_dir)
        .unwrap_or_else(|| config.modules_dir.clone());
    let modules_dir = serde_json::to_string(&modules_dir.to_string_lossy())
        .map_err(WritePnpFileError::Serialize)?;
    let contents = render_pnp_loader(&registry, &modules_dir);
    pnpm_fs::ensure_file(&lockfile_dir.join(PNP_FILENAME), contents.as_bytes(), None)
        .map_err(WritePnpFileError::Write)
}

fn render_pnp_loader(registry: &str, modules_dir: &str) -> String {
    let (before_registry, after_registry) = PNP_LOADER_TEMPLATE
        .split_once(PACKAGE_REGISTRY_PLACEHOLDER)
        .expect("embedded PnP loader has a package registry placeholder");
    let (between_values, after_modules_dir) = after_registry
        .split_once(MODULES_DIR_PLACEHOLDER)
        .expect("embedded PnP loader has a modules directory placeholder");
    let mut loader =
        String::with_capacity(PNP_LOADER_TEMPLATE.len() + registry.len() + modules_dir.len());
    loader.push_str(before_registry);
    loader.push_str(registry);
    loader.push_str(between_values);
    loader.push_str(modules_dir);
    loader.push_str(after_modules_dir);
    loader
}

#[must_use]
pub fn pnp_path_for_execution(config: &pnpm_config::Config, dir: &Path) -> Option<PathBuf> {
    let workspace_path = config.workspace_dir.as_ref().map(|dir| dir.join(PNP_FILENAME));
    if let Some(path) = workspace_path
        && path.exists()
    {
        return Some(path);
    }
    let path = dir.join(PNP_FILENAME);
    path.exists().then_some(path)
}

#[cfg(test)]
mod tests;
