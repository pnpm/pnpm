//! `pacquet publish` — publish a package to an npm registry.
//!
//! The registry-facing work (OIDC, OTP, the publish document and PUT) lives in
//! [`pnpm_publish`]; this module maps the resolved [`Config`] and CLI flags
//! onto its options, runs the git checks and publish-lifecycle scripts, and
//! packs the project before handing the tarball off.
//!
//! `--recursive` (workspace publishing), including pnpr's batch endpoint,
//! lives in [`recursive`].

pub use arguments::{PublishGitArgs, PublishManifestArgs, PublishOutputArgs, PublishRegistryArgs};
mod options;
mod recursive;
mod wait;

mod arguments;

use crate::cli_args::registry_client::build_registry_client;
use clap::Args;
use miette::{Context, IntoDiagnostic};
use pipe_trait::Pipe;
use pnpm_config::{Config, ManifestFormat};
use pnpm_executor::{RunPostinstallHooks, ScriptsPrependNodePath, run_lifecycle_hook};
use pnpm_hooks::PnpmfileHooks;
use pnpm_pack::{
    Host as PackHost, PackOptions, PackResult, WorkspacePackageManifest, api as pack_api,
};
use pnpm_publish::{
    Host, PackedPkg, PublishFailure, PublishNetwork, PublishPackedPkgOptions, PublishSummary,
    extract_publish_manifest_from_packed, is_tarball_path, publish_packed_pkg,
    resolve_otp_from_env, run_git_checks,
};
use pnpm_reporter::Reporter;
use serde_json::Value;
use std::{collections::HashMap, path::Path, sync::Arc};

/// Publish a package to the registry.
#[derive(Debug, Args)]
pub struct PublishArgs {
    /// Tarball or directory to publish. Defaults to the current directory.
    pub package: Option<String>,

    #[clap(flatten)]
    pub flags: PublishFlags,
}

/// Options controlling how a package is published.
#[derive(Debug, Args)]
pub struct PublishFlags {
    /// Do everything `publish` would do except uploading to the registry.
    #[clap(long)]
    pub dry_run: bool,
    /// Don't run publish-related lifecycle scripts.
    #[clap(long = "ignore-scripts")]
    pub ignore_scripts: bool,
    /// Publish even if the version is already in the registry.
    #[clap(long)]
    pub force: bool,
    /// Send all workspace packages in a single request (requires `--recursive`).
    #[clap(long)]
    pub batch: bool,
    #[clap(flatten)]
    pub registry: PublishRegistryArgs,
    #[clap(flatten)]
    pub manifest: PublishManifestArgs,
    #[clap(flatten)]
    pub git: PublishGitArgs,
    #[clap(flatten)]
    pub output: PublishOutputArgs,
}

/// What one `publish` / `stage publish` invocation published: the single
/// package summary, or the recursive path's per-package summaries (possibly
/// empty). The two arms serialize differently under `--json` — an object vs.
/// an array — so the split is kept rather than flattened to a `Vec`.
pub(super) enum PublishedPackages {
    Single(Box<PublishSummary>),
    Recursive(Vec<PublishSummary>),
}

struct PackedDirectory {
    project_dir: std::path::PathBuf,
    source_manifest: Value,
    published_manifest: Value,
    tarball_data: Vec<u8>,
    tarball_path: String,
    contents: Vec<String>,
    unpacked_size: u64,
}

impl PackedDirectory {
    fn packed_pkg(&self) -> PackedPkg<'_> {
        PackedPkg {
            published_manifest: &self.published_manifest,
            tarball_data: &self.tarball_data,
            tarball_path: &self.tarball_path,
            contents: &self.contents,
            unpacked_size: self.unpacked_size,
        }
    }
}

impl PublishedPackages {
    /// The summaries in publish order, without the single/recursive split.
    pub(super) fn summaries(&self) -> &[PublishSummary] {
        match self {
            PublishedPackages::Single(summary) => std::slice::from_ref(summary),
            PublishedPackages::Recursive(published) => published,
        }
    }
}

impl PublishArgs {
    /// Publish the package at `dir` (or the given tarball/directory),
    /// returning nothing — output is printed here. Handles the single-package
    /// and tarball paths.
    pub async fn run<Reporter: self::Reporter>(
        self,
        dir: &Path,
        config: &Config,
        recursive: bool,
        before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    ) -> miette::Result<()> {
        let published = self.publish_packages::<Reporter>(
            dir,
            config,
            recursive,
            /* stage */ false,
            before_packing_hooks,
        )
        .await?;
        // Mirror `pnpm publish --json`: serialize only when asked. The
        // recursive path emits the array of per-package summaries (an empty
        // array when nothing was published).
        if self.flags.output.json {
            match &published {
                PublishedPackages::Single(summary) => {
                    println!("{}", summary.pipe(serde_json::to_string_pretty).into_diagnostic()?);
                }
                PublishedPackages::Recursive(published) => {
                    println!("{}", published.pipe(serde_json::to_string_pretty).into_diagnostic()?);
                }
            }
        }
        Ok(())
    }

    /// Run the whole publish pipeline — git checks, packing, lifecycle
    /// scripts, the registry request — and return the summaries instead of
    /// printing, so `publish` and `stage publish` can render them differently.
    /// `stage` sends the upload to the registry's staging endpoint.
    pub(super) async fn publish_packages<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        recursive: bool,
        stage: bool,
        before_packing_hooks: Vec<Arc<dyn PnpmfileHooks>>,
    ) -> miette::Result<PublishedPackages> {
        self.validate_publish_flags(config, recursive, stage)?;

        // Upstream gates on `opts.gitChecks !== false`, which folds together
        // the `git-checks` config setting and the `--no-git-checks` flag.
        let publish_branch = self.flags.git.publish_branch.as_deref();
        let git_checks = config.git_checks && !self.flags.git.no_git_checks;
        run_git_checks::<Host>(dir, git_checks, publish_branch, config.ci)?;

        if recursive {
            let published =
                self.run_recursive::<Reporter>(dir, config, stage, &before_packing_hooks).await?;
            return Ok(PublishedPackages::Recursive(published));
        }

        let otp = resolve_otp_from_env::<Host>(self.flags.registry.otp.clone());
        let opts = self.publish_options(config, otp, stage);
        let http_client = build_registry_client(config)?;
        let network = PublishNetwork { client: &http_client, auth_headers: &config.auth_headers };

        let summary =
            if let Some(package) = self.package.as_deref().filter(|path| is_tarball_path(path)) {
                self.publish_tarball::<Reporter>(package, &opts, &network).await?
            } else {
                // Resolved against the command directory so every path the
                // pack derives from it — the re-anchored `file:` / `link:`
                // catalog entries among them — can be related to the
                // absolute workspace directory. `join` keeps an absolute
                // argument as it is.
                let project_dir = self.package
                    .as_deref()
                    .map_or_else(|| dir.to_path_buf(), |path| dir.join(path));
                self.publish_directory::<Reporter>(
                    &project_dir,
                    config,
                    &opts,
                    &network,
                    &before_packing_hooks,
                    None,
                )
                .await
                .map_err(|failure| failure.error)?
            };
        Ok(PublishedPackages::Single(Box::new(summary)))
    }

    /// Publish a pre-built tarball: extract its manifest and hand the bytes
    /// straight to the registry (no file listing or unpacked size).
    async fn publish_tarball<Reporter: self::Reporter>(
        &self,
        tarball_path: &str,
        opts: &PublishPackedPkgOptions,
        network: &PublishNetwork<'_>,
    ) -> miette::Result<PublishSummary> {
        let manifest = extract_publish_manifest_from_packed(tarball_path)?;
        let tarball_data = std::fs::read(tarball_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("read tarball {tarball_path}"))?;
        publish_packed_pkg::<Host, Reporter>(
            &PackedPkg {
                published_manifest: &manifest,
                tarball_data: &tarball_data,
                tarball_path,
                contents: &[],
                unpacked_size: 0,
            },
            opts,
            network,
        )
        .await
        .map_err(|failure| miette::Report::new(failure.error))
    }

    /// Publish a project directory: run `prepublishOnly` / `prepublish`, pack
    /// the project into a temporary directory, publish the tarball, then run
    /// `publish` / `postpublish`.
    async fn publish_directory<Reporter: self::Reporter>(
        &self,
        project_dir: &Path,
        config: &Config,
        opts: &PublishPackedPkgOptions,
        network: &PublishNetwork<'_>,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
        workspace_packages: Option<&Arc<HashMap<String, WorkspacePackageManifest>>>,
    ) -> Result<PublishSummary, PublishFailure<miette::Report>> {
        let packed = self.pack_directory::<Reporter>(
            project_dir,
            config,
            before_packing_hooks,
            workspace_packages,
        )
        .await?;
        let summary = publish_packed_pkg::<Host, Reporter>(&packed.packed_pkg(), opts, network)
            .await
            .map_err(|failure| PublishFailure {
                published: failure.published,
                error: miette::Report::new(failure.error),
            })?;

        self.run_post_publish_scripts::<Reporter>(&packed, config)
            .map_err(|error| PublishFailure {
                published: if opts.dry_run { Vec::new() } else { vec![summary.clone()] },
                error,
            })?;
        Ok(summary)
    }

    async fn pack_directory<Reporter: self::Reporter>(
        &self,
        project_dir: &Path,
        config: &Config,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
        workspace_packages: Option<&Arc<HashMap<String, WorkspacePackageManifest>>>,
    ) -> miette::Result<PackedDirectory> {
        let manifest = read_publish_manifest(project_dir, config.preferred_manifest_format)?;

        if !self.should_ignore_scripts(config) {
            run_publish_scripts::<Reporter>(
                project_dir,
                config,
                &manifest,
                &["prepublishOnly", "prepublish"],
            )?;
        }

        let pack_destination = tempfile::tempdir().into_diagnostic().wrap_err("create temp dir")?;
        let pack_result = self.pack_for_publish::<Reporter>(
            project_dir,
            config,
            pack_destination.path(),
            before_packing_hooks,
            workspace_packages,
        )
        .await?;
        let tarball_data = std::fs::read(&pack_result.tarball_path)
            .into_diagnostic()
            .wrap_err("read packed tarball")?;
        drop(pack_destination);

        Ok(PackedDirectory {
            project_dir: project_dir.to_path_buf(),
            source_manifest: manifest,
            published_manifest: pack_result.published_manifest,
            tarball_data,
            tarball_path: pack_result.tarball_path,
            contents: pack_result.contents,
            unpacked_size: pack_result.unpacked_size,
        })
    }

    fn run_post_publish_scripts<Reporter: self::Reporter>(
        &self,
        packed: &PackedDirectory,
        config: &Config,
    ) -> miette::Result<()> {
        if !self.should_ignore_scripts(config) {
            run_publish_scripts::<Reporter>(
                &packed.project_dir,
                config,
                &packed.source_manifest,
                &["publish", "postpublish"],
            )?;
        }
        Ok(())
    }

    /// Whether to skip every publish-related lifecycle script. `--ignore-scripts`
    /// on the publish command and the `ignore-scripts` config setting both
    /// suppress packing and publish scripts, matching pnpm's single
    /// `opts.ignoreScripts`.
    fn should_ignore_scripts(&self, config: &Config) -> bool {
        self.flags.ignore_scripts || config.ignore_scripts
    }

    /// Pack the project into `pack_destination` for publishing (never a dry
    /// run; the publish itself honors `--dry-run`).
    async fn pack_for_publish<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        pack_destination: &Path,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
        workspace_packages: Option<&Arc<HashMap<String, WorkspacePackageManifest>>>,
    ) -> miette::Result<PackResult> {
        let workspace_packages = workspace_packages
            .cloned()
            .or_else(|| {
                crate::cli_args::workspace_packages::discover_workspace_package_manifests(
                    config.workspace_dir.as_deref(),
                    config,
                )
            });
        let manifest = crate::cli_args::workspace_packages::create_publish_pack_manifest_options(
            &self.flags.manifest,
            config,
            before_packing_hooks,
            workspace_packages,
        )?;
        let mut options = PackOptions {
            dir: dir.to_path_buf(),
            workspace_dir: config.workspace_dir.clone(),
            scripts: pnpm_pack::PackScripts {
                ignore: self.should_ignore_scripts(config),
                unsafe_perm: config.unsafe_perm,
                user_agent: config.user_agent.clone(),
                extra_bin_paths: config.extra_bin_paths.clone(),
                extra_env: config.extra_env.clone(),
            },
            manifest,
            output: pnpm_pack::PackOutputOptions {
                gzip_level: None,
                dry_run: false,
                out: None,
                destination: Some(pack_destination.to_string_lossy().into_owned()),
                injected_files: Vec::new(),
                locks: None,
            },
        };
        crate::cli_args::pack::set_injected_changelog(&mut options, config, dir).await?;
        pack_api::<Reporter, PackHost>(&options).await
            .map_err(miette::Report::new)
            .wrap_err(crate::cli_args::pack::PACK_ERROR_CONTEXT)
    }
}

/// The publish manifest under `project_dir`, which must have one.
fn read_publish_manifest(
    project_dir: &Path,
    manifest_format: ManifestFormat,
) -> miette::Result<serde_json::Value> {
    pnpm_package_manifest::safe_read_project_manifest_from_dir(project_dir, manifest_format)
        .into_diagnostic()
        .wrap_err("read project manifest")?
        .ok_or_else(|| {
            let dir = project_dir.display();
            miette::miette!(
                code = "ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND",
                "No package.json found in {dir}",
            )
        })
}

/// Run the publish-lifecycle scripts the manifest declares, in order, with
/// `unsafe_perm` (publish scripts are run explicitly and assumed trusted).
fn run_publish_scripts<Reporter: self::Reporter>(
    dir: &Path,
    config: &Config,
    manifest: &Value,
    script_names: &[&str],
) -> miette::Result<()> {
    let scripts = manifest.get("scripts");
    let declares = |name: &str| {
        scripts
            .and_then(|scripts| scripts.get(name))
            .and_then(Value::as_str)
            .filter(|script| !script.is_empty())
    };
    if !script_names
        .iter()
        .any(|name| declares(name).is_some())
    {
        return Ok(());
    }

    let dep_path = dir.to_string_lossy().into_owned();
    let root_modules_dir = dir.join("node_modules");
    let run_opts = RunPostinstallHooks {
        environment: super::run::script_environment(config, dir, &config.extra_env),
        execution: pnpm_executor::ScriptExecutionOptions {
            extra_bin_paths: &config.extra_bin_paths,
            node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
            prepend_node_path: ScriptsPrependNodePath::default(),
            shell: None,
            shell_emulator: false,
            wd_bin_dir: None,
        },
        dep_path: &dep_path,
        pkg_root: dir,
        root_modules_dir: &root_modules_dir,

        unsafe_perm: true,

        optional: false,
    };
    let parent_env: HashMap<String, String> = std::env::vars().collect();

    for &name in script_names {
        let Some(script) = declares(name) else { continue };
        run_lifecycle_hook::<Reporter>(name, script, &run_opts, manifest, &parent_env)
            .map_err(miette::Report::new)
            .wrap_err_with(|| format!("run the {name} script"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
