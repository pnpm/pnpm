//! `pacquet publish` — publish a package to an npm registry.
//!
//! Ports pnpm's
//! [`publish` command](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/publish.ts).
//! The registry-facing work (OIDC, OTP, the publish document and PUT) lives in
//! [`pacquet_publish`]; this module maps the resolved [`Config`] and CLI flags
//! onto its options, runs the git checks and publish-lifecycle scripts, and
//! packs the project before handing the tarball off.
//!
//! Scope versus upstream: `--recursive` / `--batch` (workspace and batched
//! publishing) are accepted for surface parity but not yet ported — they error
//! rather than silently doing nothing.

use std::{collections::HashMap, path::Path};

use clap::Args;
use miette::{Context, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_executor::{RunPostinstallHooks, ScriptsPrependNodePath, run_lifecycle_hook};
use pacquet_pack::{Host as PackHost, PackOptions, PackResult, api as pack_api};
use pacquet_publish::{
    Access, Host, OidcHttpOptions, PackedPkg, PublishNetwork, PublishPackedPkgOptions,
    extract_manifest_from_packed, is_tarball_path, publish_packed_pkg, resolve_otp_from_env,
    run_git_checks,
};
use pacquet_reporter::Reporter;
use serde_json::Value;

use crate::cli_args::registry_client::build_registry_client;

/// `pacquet publish` arguments. The `-r` / `--recursive` selector is a global
/// flag on [`crate::CliArgs`] and threaded into [`Self::run`].
#[derive(Debug, Args)]
pub struct PublishArgs {
    /// Tarball or directory to publish. Defaults to the current directory.
    pub package: Option<String>,

    /// Do everything `publish` would do except uploading to the registry.
    #[clap(long)]
    pub dry_run: bool,

    /// Print the per-package publish summary in JSON.
    #[clap(long)]
    pub json: bool,

    /// Register the published package under this tag instead of `latest`.
    #[clap(long)]
    pub tag: Option<String>,

    /// Publish the package as `public` or `restricted`.
    #[clap(long, value_parser = ["public", "restricted"])]
    pub access: Option<String>,

    /// Generate a provenance attestation for the published package.
    #[clap(long)]
    pub provenance: bool,

    /// Don't run publish-related lifecycle scripts.
    #[clap(long = "ignore-scripts")]
    pub ignore_scripts: bool,

    /// Keep the original `packageManager` field and publish-lifecycle scripts
    /// in the published manifest instead of stripping them.
    #[clap(long = "skip-manifest-obfuscation")]
    pub skip_manifest_obfuscation: bool,

    /// One-time password for two-factor-authenticated registries.
    #[clap(long)]
    pub otp: Option<String>,

    /// The branch publishing is allowed from. Defaults to `master` / `main`.
    #[clap(long = "publish-branch")]
    pub publish_branch: Option<String>,

    /// Skip the git working-tree / branch / remote checks.
    #[clap(long = "no-git-checks")]
    pub no_git_checks: bool,

    /// Publish even if the version is already in the registry.
    #[clap(long)]
    pub force: bool,

    /// Publish all packages from the workspace.
    #[clap(long)]
    pub recursive: bool,

    /// Send all workspace packages in a single request (requires `--recursive`).
    #[clap(long)]
    pub batch: bool,
}

impl PublishArgs {
    /// Publish the package at `dir` (or the given tarball/directory),
    /// returning nothing — output is printed here. Ports pnpm's `publish`
    /// handler for the single-package and tarball paths.
    pub async fn run<Reporter: self::Reporter>(
        self,
        dir: &Path,
        config: &Config,
        recursive: bool,
    ) -> miette::Result<()> {
        if self.batch && !(recursive || self.recursive) {
            return Err(miette::miette!(
                code = "ERR_PNPM_BATCH_PUBLISH_REQUIRES_RECURSIVE",
                "--batch can only be used together with --recursive",
            ));
        }

        let publish_branch = self.publish_branch.as_deref();
        run_git_checks::<Host>(dir, !self.no_git_checks, publish_branch)?;

        if recursive || self.recursive {
            return Err(miette::miette!(
                code = "ERR_PNPM_RECURSIVE_PUBLISH_UNSUPPORTED",
                help =
                    "Run `pnpm publish` in each package directory, or use the TypeScript pnpm CLI.",
                "Recursive publishing is not yet supported by pacquet",
            ));
        }

        let otp = resolve_otp_from_env::<Host>(self.otp.clone());
        let opts = self.publish_options(config, otp);
        let http_client = build_registry_client(config)?;
        let network = PublishNetwork { client: &http_client, auth_headers: &config.auth_headers };

        let summary_json =
            if let Some(package) = self.package.as_deref().filter(|p| is_tarball_path(p)) {
                self.publish_tarball::<Reporter>(package, &opts, &network).await?
            } else {
                self.publish_directory::<Reporter>(
                    self.package.as_deref().unwrap_or_else(|| dir_str(dir)),
                    config,
                    &opts,
                    &network,
                )
                .await?
            };

        if self.json {
            println!("{summary_json}");
        }
        Ok(())
    }

    /// Publish a pre-built tarball: extract its manifest and hand the bytes
    /// straight to the registry (no file listing or unpacked size).
    async fn publish_tarball<Reporter: self::Reporter>(
        &self,
        tarball_path: &str,
        opts: &PublishPackedPkgOptions,
        network: &PublishNetwork<'_>,
    ) -> miette::Result<String> {
        let manifest = extract_manifest_from_packed(tarball_path)?;
        let tarball_data = std::fs::read(tarball_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("read tarball {tarball_path}"))?;
        let summary = publish_packed_pkg::<Host, Reporter>(
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
        .await?;
        serde_json::to_string_pretty(&summary).into_diagnostic()
    }

    /// Publish a project directory: run `prepublishOnly` / `prepublish`, pack
    /// the project into a temporary directory, publish the tarball, then run
    /// `publish` / `postpublish`.
    async fn publish_directory<Reporter: self::Reporter>(
        &self,
        dir: &str,
        config: &Config,
        opts: &PublishPackedPkgOptions,
        network: &PublishNetwork<'_>,
    ) -> miette::Result<String> {
        let project_dir = Path::new(dir);
        let manifest = pacquet_package_manifest::safe_read_package_json_from_dir(project_dir)
            .into_diagnostic()
            .wrap_err("read package.json")?
            .ok_or_else(|| {
                miette::miette!(
                    code = "ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND",
                    "No package.json found in {dir}",
                )
            })?;

        if !self.ignore_scripts {
            run_publish_scripts::<Reporter>(
                project_dir,
                config,
                &manifest,
                &["prepublishOnly", "prepublish"],
            )?;
        }

        let pack_destination = tempfile::tempdir().into_diagnostic().wrap_err("create temp dir")?;
        let pack_result =
            self.pack_for_publish::<Reporter>(project_dir, config, pack_destination.path())?;
        let tarball_data = std::fs::read(&pack_result.tarball_path)
            .into_diagnostic()
            .wrap_err("read packed tarball")?;

        let summary = publish_packed_pkg::<Host, Reporter>(
            &PackedPkg {
                published_manifest: &pack_result.published_manifest,
                tarball_data: &tarball_data,
                tarball_path: &pack_result.tarball_path,
                contents: &pack_result.contents,
                unpacked_size: pack_result.unpacked_size,
            },
            opts,
            network,
        )
        .await?;
        drop(pack_destination);

        if !self.ignore_scripts {
            run_publish_scripts::<Reporter>(
                project_dir,
                config,
                &manifest,
                &["publish", "postpublish"],
            )?;
        }
        serde_json::to_string_pretty(&summary).into_diagnostic()
    }

    /// Pack the project into `pack_destination` for publishing (never a dry
    /// run; the publish itself honors `--dry-run`).
    fn pack_for_publish<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        pack_destination: &Path,
    ) -> miette::Result<PackResult> {
        let options = PackOptions {
            dir: dir.to_path_buf(),
            catalogs: config.catalogs.clone().unwrap_or_default(),
            ignore_scripts: config.ignore_scripts,
            unsafe_perm: config.unsafe_perm,
            embed_readme: false,
            pack_gzip_level: None,
            node_linker: config.node_linker,
            skip_manifest_obfuscation: self.skip_manifest_obfuscation,
            user_agent: config.user_agent.clone(),
            extra_bin_paths: config.extra_bin_paths.clone(),
            extra_env: HashMap::new(),
            workspace_dir: config.workspace_dir.clone(),
            dry_run: false,
            out: None,
            pack_destination: Some(pack_destination.to_string_lossy().into_owned()),
        };
        pack_api::<Reporter, PackHost>(&options)
            .map_err(miette::Report::new)
            .wrap_err("pack the package")
    }

    /// Map the CLI flags and resolved [`Config`] onto the publish options.
    fn publish_options(&self, config: &Config, otp: Option<String>) -> PublishPackedPkgOptions {
        PublishPackedPkgOptions {
            default_registry: config.registry.clone(),
            scoped_registries: config.registries.clone(),
            access: self.access.as_deref().and_then(Access::parse),
            tag: self.tag.clone().unwrap_or_else(|| "latest".to_owned()),
            otp,
            // An absent `--provenance` leaves the decision to the OIDC flow.
            provenance: self.provenance.then_some(true),
            dry_run: self.dry_run,
            stage: false,
            http: OidcHttpOptions {
                fetch_retries: Some(config.fetch_retries),
                fetch_retry_factor: Some(f64::from(config.fetch_retry_factor)),
                fetch_retry_maxtimeout: Some(config.fetch_retry_maxtimeout),
                fetch_retry_mintimeout: Some(config.fetch_retry_mintimeout),
                fetch_timeout: Some(config.fetch_timeout),
            },
        }
    }
}

/// Run the publish-lifecycle scripts the manifest declares, in order, with
/// `unsafe_perm` (publish scripts are run explicitly and assumed trusted).
/// Mirrors pnpm's `runScriptsIfPresent`.
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
            .filter(|s| !s.is_empty())
    };
    if !script_names.iter().any(|name| declares(name).is_some()) {
        return Ok(());
    }

    let dep_path = dir.to_string_lossy().into_owned();
    let root_modules_dir = dir.join("node_modules");
    let run_opts = RunPostinstallHooks {
        dep_path: &dep_path,
        pkg_root: dir,
        root_modules_dir: &root_modules_dir,
        init_cwd: dir,
        extra_bin_paths: &config.extra_bin_paths,
        extra_env: &HashMap::new(),
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: Some(&config.user_agent),
        unsafe_perm: true,
        node_gyp_bin: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::default(),
        script_shell: None,
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

/// Borrow `dir` as a `&str` for the default-directory case.
fn dir_str(dir: &Path) -> &str {
    dir.to_str().unwrap_or(".")
}
