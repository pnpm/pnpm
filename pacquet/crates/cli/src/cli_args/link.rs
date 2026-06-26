use crate::State;
use clap::Args;
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Context, Diagnostic};
use pacquet_config::Config;
use pacquet_package_manager::{Install, UpdateSeedPolicy};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::Reporter;
use pacquet_workspace_manifest_writer::set_overrides;
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
    format!("link:{}", rel.display().to_string().replace('\\', "/"))
}

fn already_declared(manifest: &PackageManifest, name: &str) -> bool {
    DEPENDENCY_FIELDS.iter().any(|field| {
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
        if self.package_paths.is_empty() {
            return Err(LinkError::NoParams.into());
        }

        if let Some(name) = self.package_paths.iter().find(|path| !is_filespec(path)) {
            return Err(LinkError::LinkByName { name: name.clone() }.into());
        }

        let manifest_dir = manifest_path
            .parent()
            .ok_or_else(|| miette::miette!("manifest path has no parent directory"))?
            .to_path_buf();

        let mut manifest = PackageManifest::create_if_needed(manifest_path.clone())
            .wrap_err("reading the project package.json")?;

        let root_dir = config.workspace_dir.clone().unwrap_or_else(|| manifest_dir.clone());

        let mut new_overrides = IndexMap::<String, String>::new();
        for path_str in &self.package_paths {
            let target_path = PathBuf::from(path_str);
            let target_dir = if target_path.is_absolute() {
                target_path.clone()
            } else {
                manifest_dir.join(&target_path)
            };

            let target_manifest_path = target_dir.join("package.json");
            let dir_display = target_dir.display();
            let target_manifest = PackageManifest::from_path(target_manifest_path)
                .map_err(|_| miette::miette!("No package.json found in {}", dir_display))?;
            let package_name = target_manifest.value()["name"]
                .as_str()
                .ok_or_else(|| miette::miette!("Target package does not have a name field"))?
                .to_string();

            if !already_declared(&manifest, &package_name) {
                manifest
                    .add_dependency(
                        &package_name,
                        &link_spec(&manifest_dir, &target_dir),
                        DependencyGroup::Prod,
                    )
                    .wrap_err("adding linked dependency to package.json")?;
            }
            new_overrides.insert(package_name, link_spec(&root_dir, &target_dir));
        }

        manifest.save().wrap_err("saving package.json with linked dependencies")?;

        set_overrides(&root_dir, new_overrides.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .wrap_err("recording linked dependencies in pnpm-workspace.yaml")?;

        let overrides = config.overrides.get_or_insert_with(IndexMap::new);
        for (selector, specifier) in &new_overrides {
            overrides.insert(selector.clone(), specifier.clone());
        }

        let state = State::init(manifest_path, config, false).wrap_err("initialize the state")?;

        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;

        let lockfile_path = manifest
            .path()
            .parent()
            .map(|parent| parent.join(pacquet_lockfile::Lockfile::FILE_NAME));

        Install {
            tarball_mem_cache: Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: Arc::clone(http_client),
            config,
            manifest,
            lockfile: pacquet_lockfile::MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: lockfile_path.as_deref(),
            dependency_groups: [
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
            ]
            .into_iter(),
            frozen_lockfile: false,
            prefer_frozen_lockfile: Some(false),
            ignore_manifest_check: false,
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            is_full_install: false,
            resolved_packages,
            supported_architectures: config.supported_architectures.clone(),
            node_linker: config.node_linker,
            lockfile_only: false,
            dry_run: false,
            disable_optimistic_repeat_install: false,
            update_seed_policy: UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            catalogs_override: None,
        }
        .run::<Reporter>()
        .await
        .wrap_err("linking dependencies")
    }
}
