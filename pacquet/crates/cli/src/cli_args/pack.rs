//! `pacquet pack` — create a tarball from a package.
//!
//! Ports pnpm's
//! [`pack` command](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/pack.ts).
//! The single-project work lives in [`pacquet_pack::api`]; this module
//! maps the resolved [`Config`] and CLI flags onto
//! [`pacquet_pack::PackOptions`], and drives the recursive (`-r`) sweep
//! over the workspace the same way the other recursive commands do.
//!
//! Scope versus upstream: `--workspace-concurrency` is accepted but the
//! recursive sweep runs sequentially (matching pacquet's other
//! recursive commands), and the `embedReadme` / `extraEnv` config keys
//! are not surfaced by `Config` yet, so they take their `false` / empty
//! defaults.

use crate::cli_args::recursive::{
    discover_workspace_projects, select_recursive_projects, sort_projects,
};
use clap::Args;
use miette::Context;
use pacquet_config::Config;
use pacquet_pack::{
    Host, PackError, PackOptions, PackResultJson, api, format_pack_output, to_pack_result_json,
};
use pacquet_reporter::Reporter;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// `pacquet pack` arguments. The `-r` / `--recursive` and `--filter`
/// selectors are global flags on [`crate::CliArgs`]; `--recursive` is
/// threaded into [`Self::run`].
#[derive(Debug, Args)]
pub struct PackArgs {
    /// Do everything `pack` would do except writing the tarball to disk.
    #[clap(long)]
    pub dry_run: bool,

    /// Directory in which to save the tarball. Defaults to the current
    /// working directory.
    #[clap(long = "pack-destination")]
    pub pack_destination: Option<String>,

    /// Print the packed tarball and its contents in JSON.
    #[clap(long)]
    pub json: bool,

    /// Customize the output path. `%s` expands to the package name and
    /// `%v` to the version, e.g. `%s.tgz` or `some-dir/%s-%v.tgz`.
    #[clap(long)]
    pub out: Option<String>,

    /// gzip compression level (`0`–`9`) for the tarball.
    #[clap(long = "pack-gzip-level", value_parser = clap::value_parser!(u32).range(0..=9))]
    pub pack_gzip_level: Option<u32>,

    /// Keep the original `packageManager` field and publish-lifecycle
    /// scripts in the packed manifest instead of stripping them.
    #[clap(long = "skip-manifest-obfuscation")]
    pub skip_manifest_obfuscation: bool,

    /// Maximum number of projects packed at once in recursive mode.
    /// Accepted for surface parity; the sweep currently runs
    /// sequentially.
    #[clap(long = "workspace-concurrency")]
    pub workspace_concurrency: Option<u32>,
}

impl PackArgs {
    /// Pack the project at `dir` (or every workspace project when
    /// `recursive`), returning the text/JSON the CLI prints.
    pub fn run<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
        recursive: bool,
    ) -> miette::Result<String> {
        if recursive {
            self.run_recursive::<Reporter>(dir, config)
        } else {
            let options = self.pack_options(
                dir.to_path_buf(),
                config,
                self.out.clone(),
                self.pack_destination.clone(),
            );
            let result = api::<Reporter, Host>(&options)
                .map_err(miette::Report::new)
                .wrap_err("pack the package")?;
            Ok(format_pack_output(&[to_pack_result_json(&result)], self.json, false))
        }
    }

    /// Pack every workspace project that declares both a name and a
    /// version, in topological order. Mirrors the recursive arm of
    /// pnpm's `handler`.
    fn run_recursive<Reporter: self::Reporter>(
        &self,
        dir: &Path,
        config: &Config,
    ) -> miette::Result<String> {
        // `--out` and `--pack-destination` are mutually exclusive. The
        // single-project path enforces this inside `api`; the recursive
        // path resolves a shared destination before `api` ever sees both,
        // so check here too rather than silently dropping one.
        if self.out.is_some() && self.pack_destination.is_some() {
            return Err(miette::Report::new(PackError::OutAndPackDestination));
        }
        let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
        let projects = discover_workspace_projects(workspace_root)?;
        let graph = select_recursive_projects(&projects, config, dir)?;
        let chunks = sort_projects(&graph);

        // In recursive mode upstream resolves `--out` / `--pack-destination`
        // to an absolute path against the CLI dir (and defaults the
        // destination to the CLI dir), so every tarball lands in one place
        // regardless of each project's own root.
        let (out, pack_destination) = self.resolve_recursive_destination(dir);

        let mut packed: Vec<PackResultJson> = Vec::new();
        for chunk in &chunks {
            for root in chunk {
                let project = graph[root].package.project;
                let manifest = project.manifest.value();
                let has_name = manifest
                    .get("name")
                    .and_then(|name| name.as_str())
                    .is_some_and(|name| !name.is_empty());
                let has_version = manifest
                    .get("version")
                    .and_then(|version| version.as_str())
                    .is_some_and(|version| !version.is_empty());
                if !has_name || !has_version {
                    continue;
                }
                let options = self.pack_options(
                    project.root_dir.clone(),
                    config,
                    out.clone(),
                    pack_destination.clone(),
                );
                let result = api::<Reporter, Host>(&options)
                    .map_err(miette::Report::new)
                    .wrap_err_with(|| format!("pack {}", project.root_dir.display()))?;
                packed.push(to_pack_result_json(&result));
            }
        }

        if packed.is_empty() {
            tracing::info!(
                target: "pacquet::pack",
                prefix = %dir.display(),
                "There are no packages that should be packed",
            );
            return Ok(String::new());
        }
        Ok(format_pack_output(&packed, self.json, false))
    }

    /// Resolve the recursive-mode `(out, pack_destination)` pair to
    /// absolute paths against the CLI `dir`.
    fn resolve_recursive_destination(&self, dir: &Path) -> (Option<String>, Option<String>) {
        if let Some(out) = &self.out {
            (Some(absolute_against(dir, out)), None)
        } else if let Some(destination) = &self.pack_destination {
            (None, Some(absolute_against(dir, destination)))
        } else {
            (None, Some(dir.to_string_lossy().into_owned()))
        }
    }

    /// Map `self` plus the resolved `config` onto a [`PackOptions`].
    fn pack_options(
        &self,
        dir: PathBuf,
        config: &Config,
        out: Option<String>,
        pack_destination: Option<String>,
    ) -> PackOptions {
        PackOptions {
            dir,
            catalogs: config.catalogs.clone().unwrap_or_default(),
            ignore_scripts: config.ignore_scripts,
            unsafe_perm: config.unsafe_perm,
            embed_readme: false,
            pack_gzip_level: self.pack_gzip_level,
            node_linker: config.node_linker,
            skip_manifest_obfuscation: self.skip_manifest_obfuscation,
            user_agent: config.user_agent.clone(),
            extra_bin_paths: config.extra_bin_paths.clone(),
            extra_env: HashMap::new(),
            workspace_dir: config.workspace_dir.clone(),
            dry_run: self.dry_run,
            out,
            pack_destination,
        }
    }
}

/// Resolve `path` against `base` when it is relative, mirroring node's
/// `path.resolve(base, path)`.
fn absolute_against(base: &Path, path: &str) -> String {
    if Path::new(path).is_absolute() {
        path.to_string()
    } else {
        base.join(path).to_string_lossy().into_owned()
    }
}
