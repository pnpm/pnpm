use crate::{
    cargo_deps,
    cargo_manifest,
    cargo_manifest::CargoDependencyKind,
    ecosystem_install::InstallContext,
    package_specifier::RegistryPackageSpecifier,
};
use futures_util::{
    StreamExt,
    TryStreamExt,
    stream,
};
use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
};
use pnpm_config::Config;
use pnpm_install_coordinator::InstallTask;
use pnpm_network::ThrottledClient;
use std::{
    fs,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

pub(crate) struct AddOptions {
    pub(crate) packages: Vec<RegistryPackageSpecifier>,
    pub(crate) dependency_kind: CargoDependencyKind,
    pub(crate) save_exact: bool,
    pub(crate) save_prefix: Option<String>,
}

pub(crate) async fn plan<Reporter: pnpm_reporter::Reporter + 'static>(
    context: InstallContext,
    manifest_path: PathBuf,
    options: AddOptions,
) -> Result<(PathBuf, InstallTask<'static>)> {
    if !context.config.cargo.enabled {
        return Err(miette::miette!(
            "crate: dependencies require `cargo.enabled: true` in pnpm-workspace.yaml"
        ));
    }
    if !manifest_path.is_file() {
        let manifest_path = manifest_path.display();
        return Err(miette::miette!("cannot add a crate because {manifest_path} does not exist"));
    }
    ensure_package_manifest(&manifest_path)?;
    let manifest_dir = manifest_path.parent().unwrap_or(&manifest_path);
    let toolchain_checkout =
        cargo_deps::checkout(context.config.workspace_dir.as_deref().unwrap_or(manifest_dir));
    super::toolchain::provision::<Reporter>(
        context.config,
        &context.http_client,
        std::slice::from_ref(&manifest_path),
        toolchain_checkout.as_deref(),
    )
    .await?;
    let root = cargo_deps::workspace_root(&manifest_path, toolchain_checkout.as_deref()).await?;
    let checkout = cargo_deps::checkout(context.config.workspace_dir.as_deref().unwrap_or(&root));
    let mut metadata = cargo_deps::metadata_paths(&root).to_vec();
    metadata.push(manifest_path.clone());
    let transaction_root = root.clone();
    let prepare = async move {
        prepare_manifest(context.config, &manifest_path, options, Arc::clone(&context.http_client))
            .await?;
        cargo_deps::prepare::<Reporter>(
            context,
            vec![root],
            checkout,
            cargo_deps::CargoLockfilePolicy::KeepLocked,
        )
        .await
    };
    Ok((transaction_root, InstallTask::new(metadata, prepare)))
}

async fn prepare_manifest(
    config: &Config,
    cargo_manifest_path: &Path,
    options: AddOptions,
    http_client: Arc<ThrottledClient>,
) -> Result<()> {
    let AddOptions {
        packages,
        dependency_kind,
        save_exact,
        save_prefix,
    } = options;
    let save_prefix = save_prefix.as_deref();
    let auth_headers = packages
        .iter()
        .any(|package| package.version_spec.is_none())
        .then(|| cargo_deps::cargo_auth_headers(config))
        .transpose()?;

    let resolved = stream::iter(packages)
        .map(|package| {
            let http_client = Arc::clone(&http_client);
            let auth_headers = auth_headers.clone();
            async move {
                let version_spec =
                    if let Some(version_spec) = package.version_spec.as_deref() {
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
    cargo_manifest::add_dependencies(cargo_manifest_path, &resolved, dependency_kind)
}

/// Fail on a virtual manifest, a `[workspace]` with no `[package]`: Cargo
/// rejects dependency tables there, as `cargo add` does.
fn ensure_package_manifest(manifest_path: &Path) -> Result<()> {
    let contents = fs::read_to_string(manifest_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", manifest_path.display()))?;
    let manifest = toml::from_str::<toml::Table>(&contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse {}", manifest_path.display()))?;
    if manifest.contains_key("package") || !manifest.contains_key("workspace") {
        return Ok(());
    }
    let manifest_path = manifest_path.display();
    Err(miette::miette!(
        code = "ERR_PNPM_CARGO_VIRTUAL_MANIFEST",
        help = "Run the command in the directory of a workspace member.",
        "cannot add a crate to {manifest_path}, a virtual Cargo workspace manifest with no [package]",
    ))
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
