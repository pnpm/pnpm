use crate::State;
use clap::Args;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Context, Diagnostic};
use pnpm_config::Config;
use pnpm_package_manager::{Install, ProjectMutation};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_workspace_manifest_writer::set_overrides;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Links a local package as a dependency.
#[derive(Debug, Args)]
pub struct LinkArgs {
    pub package_paths: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LinkError {
    #[display("You must provide a parameter. Usage: pnpm link <dir>")]
    #[diagnostic(code(ERR_PNPM_LINK_BAD_PARAMS))]
    NoParams,

    #[display(r#"Cannot link by package name. Use a relative or absolute path instead, e.g. "pnpm link ./{name}""#)]
    #[diagnostic(code(ERR_PNPM_LINK_BAD_PARAMS))]
    LinkByName {
        #[error(not(source))]
        name: String,
    },
}

const DEPENDENCY_FIELDS: [&str; 3] = ["optionalDependencies", "dependencies", "devDependencies"];

fn is_filespec(input: &str) -> bool {
    let mut chars = input.chars();
    match chars.next() {
        Some('.' | '/') => true,
        Some('\\') if cfg!(windows) => true,
        Some('~') => chars.next() == Some('/'),
        Some(c) if c.is_ascii_alphabetic() => chars.next() == Some(':'),
        _ => false,
    }
}

fn link_spec(base: &Path, target: &Path) -> String {
    let rel = pathdiff::diff_paths(target, base).unwrap_or_else(|| target.to_path_buf());
    format!(
        "link:{}",
        rel.display()
            .to_string()
            .replace('\\', "/"),
    )
}

fn already_declared(manifest: &PackageManifest, name: &str) -> bool {
    DEPENDENCY_FIELDS
        .iter()
        .any(|field| {
            manifest
                .value()
                .get(field)
                .and_then(serde_json::Value::as_object)
                .is_some_and(|deps| deps.contains_key(name))
        })
}

impl LinkArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(
        self,
        config: &'static mut Config,
        manifest_path: PathBuf,
    ) -> miette::Result<()> {
        self.validate_package_paths()?;

        let manifest_dir = manifest_path
            .parent()
            .ok_or_else(|| miette::miette!("manifest path has no parent directory"))?
            .to_path_buf();

        let mut manifest = PackageManifest::create_if_needed(manifest_path.clone())
            .wrap_err("reading the project package.json")?;

        let root_dir = config.workspace_dir.clone().unwrap_or_else(|| manifest_dir.clone());

        let mut new_overrides = IndexMap::<String, String>::new();
        for path_str in &self.package_paths {
            let (target_dir, package_name, target_manifest) = link_target(&manifest_dir, path_str)?;

            if !already_declared(&manifest, &package_name) {
                manifest
                    .add_dependency(
                        &package_name,
                        &link_spec(&manifest_dir, &target_dir),
                        DependencyGroup::Prod,
                    )
                    .wrap_err("adding linked dependency to package.json")?;
            }
            new_overrides.insert(package_name.clone(), link_spec(&root_dir, &target_dir));
            check_peer_deps::<Reporter>(&package_name, &target_manifest, &manifest_dir);
        }

        manifest.save().wrap_err("saving package.json with linked dependencies")?;

        set_overrides(
            &root_dir,
            new_overrides
                .iter()
                .map(|(selector, specifier)| (selector.as_str(), specifier.as_str())),
        )
        .wrap_err("recording linked dependencies in pnpm-workspace.yaml")?;

        config.overrides
            .get_or_insert_with(IndexMap::new)
            .extend(
                new_overrides
                    .iter()
                    .map(|(selector, specifier)| (selector.clone(), specifier.clone())),
            );

        let state = State::init(manifest_path, config, false).wrap_err("initialize the state")?;
        install_linked::<Reporter>(&state).await
    }

    fn validate_package_paths(&self) -> miette::Result<()> {
        if self.package_paths.is_empty() {
            return Err(LinkError::NoParams.into());
        }

        if let Some(name) = self.package_paths
            .iter()
            .find(|path| !is_filespec(path))
        {
            return Err(LinkError::LinkByName { name: name.clone() }.into());
        }

        Ok(())
    }
}

/// Install with the linked dependencies' overrides in place.
async fn install_linked<Reporter: self::Reporter + 'static>(state: &State) -> miette::Result<()> {
    let lockfile_path = state.lockfile_path();
    {
        let mut base_install = Install::new(
            Arc::clone(&state.tarball_mem_cache),
            &state.resolved_packages,
            (&state.http_client, Arc::clone(&state.http_client)),
            state.config,
            &state.manifest,
            pnpm_lockfile::MaybeLazyLockfile::Lazy(&state.lockfile),
            [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional].into_iter(),
        );
        base_install.lockfile_policy.prefer_frozen = Some(false);
        base_install.execution.mutation = ProjectMutation::NoInstall;
        base_install.execution.installs_only = false;
        base_install.context.lockfile_path = Some(&lockfile_path);
        base_install
    }
    .run::<Reporter>()
    .await
    .wrap_err("linking dependencies")
}

/// The linked package's directory, the name it is declared under, and its manifest.
fn link_target(
    manifest_dir: &Path,
    path_str: &str,
) -> miette::Result<(PathBuf, String, PackageManifest)> {
    let target_path = PathBuf::from(path_str);
    let target_dir =
        if target_path.is_absolute() { target_path } else { manifest_dir.join(&target_path) };
    let target_manifest_path = pnpm_workspace::project_manifest_path(&target_dir);
    let dir_display = target_dir.display();
    let target_manifest = PackageManifest::from_path(target_manifest_path)
        .map_err(|error| match error {
            pnpm_package_manifest::PackageManifestError::NoImporterManifestFound(_) => {
                miette::miette!("No package.json found in {}", dir_display)
            }
            error => miette::Report::new(error),
        })?;
    let package_name = target_manifest.value()["name"]
        .as_str()
        .ok_or_else(|| miette::miette!("Target package does not have a name field"))?
        .to_string();
    Ok((target_dir, package_name, target_manifest))
}

fn check_peer_deps<Reporter: self::Reporter>(
    package_name: &str,
    target_manifest: &PackageManifest,
    prefix: &Path,
) {
    if let Some(peer_deps_map) = target_manifest
        .value()
        .get("peerDependencies")
        .and_then(serde_json::Value::as_object)
        && !peer_deps_map.is_empty()
    {
        let peer_deps = peer_deps_map
            .iter()
            .map(|(key, value)| {
                let val_str = value.as_str().map_or_else(|| value.to_string(), ToString::to_string);
                format!("  - {key}@{val_str}")
            })
            .collect::<Vec<_>>()
            .join(", ");

        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: format!(
                "The package {package_name}, which you have just pnpm linked, has the following peerDependencies specified in its package.json:\n\n{peer_deps}\n\nThe linked in dependency will not resolve the peer dependencies from the target node_modules.\nThis might cause issues in your project. To resolve this, you may use the \"file:\" protocol to reference the local dependency.",
            ),
            prefix: prefix.display().to_string(),
        }));
    }
}
