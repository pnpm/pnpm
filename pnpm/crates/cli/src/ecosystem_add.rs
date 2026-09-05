use crate::{
    cargo_deps,
    cargo_manifest::{self, CargoDependencyKind},
    package_specifier::EcosystemPackageSpecifier,
};
use futures_util::{StreamExt, TryStreamExt, stream};
use miette::Result;
use pnpm_config::Config;
use pnpm_network::ThrottledClient;
use std::{path::Path, sync::Arc};

/// Update supported non-Node.js manifests for an add request.
pub(crate) async fn prepare(
    config: &Config,
    cargo_manifest_path: &Path,
    packages: &[EcosystemPackageSpecifier],
    cargo_dependency_kind: CargoDependencyKind,
    save_exact: bool,
    save_prefix: Option<&str>,
    http_client: Arc<ThrottledClient>,
) -> Result<()> {
    let cargo_packages = packages
        .iter()
        .map(|package| match package {
            EcosystemPackageSpecifier::Cargo(package) => package.clone(),
        })
        .collect::<Vec<_>>();
    if cargo_packages.is_empty() {
        return Ok(());
    }
    if !config.cargo.enabled {
        return Err(miette::miette!(
            "crate: dependencies require `cargo.enabled: true` in pnpm-workspace.yaml"
        ));
    }
    if !cargo_manifest_path.is_file() {
        let cargo_manifest_path = cargo_manifest_path.display();
        return Err(miette::miette!(
            "cannot add a crate because {} does not exist",
            cargo_manifest_path,
        ));
    }

    let auth_headers = cargo_packages
        .iter()
        .any(|package| package.version_spec.is_none())
        .then(|| cargo_deps::crates_io_auth_headers(config))
        .transpose()?;

    let resolved = stream::iter(cargo_packages)
        .map(|package| {
            let http_client = Arc::clone(&http_client);
            let auth_headers = auth_headers.clone();
            async move {
                let version_spec = if let Some(version_spec) = package.version_spec.as_deref() {
                    version_spec.to_string()
                } else {
                    let auth_headers = auth_headers
                        .as_deref()
                        .expect("auth is prepared when a version lookup is needed");
                    let version = cargo_deps::latest_version(
                        config,
                        auth_headers,
                        &package.name,
                        &http_client,
                    )
                    .await?;
                    saved_version(&version, save_exact, save_prefix)
                };
                Ok::<_, miette::Report>((package.name.clone(), version_spec))
            }
        })
        .buffer_unordered(config.network_concurrency.clamp(1, 16))
        .try_collect::<Vec<_>>()
        .await?;
    cargo_manifest::add_dependencies(cargo_manifest_path, &resolved, cargo_dependency_kind)
}

fn saved_version(version: &str, save_exact: bool, save_prefix: Option<&str>) -> String {
    if save_exact {
        format!("={version}")
    } else {
        format!("{}{version}", save_prefix.unwrap_or(""))
    }
}

#[cfg(test)]
mod tests;
