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
mod recursive;

mod arguments;

use crate::cli_args::{install::resolve_bool_override, registry_client::build_registry_client};
use clap::Args;
use miette::{Context, IntoDiagnostic};
use pipe_trait::Pipe;
use pnpm_config::Config;
use pnpm_executor::{RunPostinstallHooks, ScriptsPrependNodePath, run_lifecycle_hook};
use pnpm_hooks::PnpmfileHooks;
use pnpm_pack::{Host as PackHost, PackOptions, PackResult, api as pack_api};
use pnpm_publish::{
    Access, Host, OidcHttpOptions, PackedPkg, PublishNetwork, PublishPackedPkgOptions,
    PublishSummary, extract_publish_manifest_from_packed, is_tarball_path, publish_packed_pkg,
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
        if self.flags.batch && !recursive {
            return Err(miette::miette!(
                code = "ERR_PNPM_BATCH_PUBLISH_REQUIRES_RECURSIVE",
                help = r#"Run "pnpm publish -r --batch" to publish all workspace packages in a single request."#,
                "--batch can only be used together with --recursive",
            ));
        }

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
                )
                .await?
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
        .map_err(miette::Report::new)
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
    ) -> miette::Result<PublishSummary> {
        let packed =
            self.pack_directory::<Reporter>(project_dir, config, before_packing_hooks).await?;
        let summary =
            publish_packed_pkg::<Host, Reporter>(&packed.packed_pkg(), opts, network).await?;

        self.run_post_publish_scripts::<Reporter>(&packed, config)?;
        Ok(summary)
    }

    async fn pack_directory<Reporter: self::Reporter>(
        &self,
        project_dir: &Path,
        config: &Config,
        before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
    ) -> miette::Result<PackedDirectory> {
        let manifest = pnpm_package_manifest::safe_read_package_json_from_dir(project_dir)
            .into_diagnostic()
            .wrap_err("read package.json")?
            .ok_or_else(|| {
                let dir = project_dir.display();
                miette::miette!(
                    code = "ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND",
                    "No package.json found in {dir}",
                )
            })?;

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
    ) -> miette::Result<PackResult> {
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
            manifest: pnpm_pack::PackManifestOptions {
                catalogs: crate::cli_args::catalogs::configured_catalogs(config)?,
                catalogs_dir: config.workspace_dir.clone(),
                embed_readme: resolve_bool_override(
                    self.flags.manifest.embed_readme,
                    self.flags.manifest.no_embed_readme,
                    config.embed_readme,
                ),
                node_linker: config.node_linker,
                skip_obfuscation: resolve_bool_override(
                    self.flags.manifest.skip_manifest_obfuscation,
                    self.flags.manifest.no_skip_manifest_obfuscation,
                    config.skip_manifest_obfuscation,
                ),
                before_packing_hooks: before_packing_hooks.to_vec(),
            },
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

    /// Map the CLI flags and resolved [`Config`] onto the publish options.
    fn publish_options(
        &self,
        config: &Config,
        otp: Option<String>,
        stage: bool,
    ) -> PublishPackedPkgOptions {
        PublishPackedPkgOptions {
            dry_run: self.flags.dry_run,
            stage,
            registry: pnpm_publish::PublishRegistryOptions {
                default: config.registry.clone(),
                scoped: config.registries_by_scope.clone(),
                access: self.flags.registry.access.as_deref().and_then(Access::parse),
                tag: self.flags.registry.tag.clone().unwrap_or_else(|| "latest".to_owned()),
                otp,
                // An absent `--provenance` leaves the decision to the OIDC flow.
                provenance: self.flags.registry.provenance.then_some(true),
                http: OidcHttpOptions {
                    fetch_retries: Some(config.fetch_retries),
                    fetch_retry_factor: Some(f64::from(config.fetch_retry_factor)),
                    fetch_retry_maxtimeout: Some(config.fetch_retry_maxtimeout),
                    fetch_retry_mintimeout: Some(config.fetch_retry_mintimeout),
                    fetch_timeout: Some(config.fetch_timeout),
                },
            },
        }
    }
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
