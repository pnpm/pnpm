//! `pacquet pack-app`.
//!
//! Packs a `CommonJS` entry file into a standalone executable for one or
//! more target platforms, embedding a Node.js binary through the Node.js
//! [Single Executable Applications API](https://nodejs.org/api/single-executable-applications.html).
//!
//! Two behaviors are forced by pacquet being a Rust binary rather than a
//! Node.js script:
//!
//! - **The SEA builder is always downloaded.** pnpm reuses its own
//!   running interpreter (`process.execPath`) when it already matches the
//!   embedded runtime version. pacquet has no host Node.js to reuse, so it
//!   always fetches a host-arch Node.js of the embedded runtime version to
//!   run `--build-sea`.
//! - **The runtime install spawns the pacquet binary.** pacquet
//!   re-invokes itself (`std::env::current_exe()`) running
//!   `add node@runtime:<version>` with the target `--os` / `--cpu` /
//!   `--libc` flags into an isolated install directory under the pnpm
//!   home.

pub use errors::PackAppError;

use build::{
    SeaBuild, ad_hoc_sign_mac_binary, ensure_node_runtime, pnpm_home_dir, print_built,
    reject_non_regular_output_file, reject_non_regular_outputs, resolve_builder_binary,
    resolve_version, run_command,
};
use clap::Args;
use config::{
    ParsedTarget, ReadProjectAppConfigResult, default_runtime_version,
    derive_output_name_from_package, escapes_project, output_file_name, parse_runtime,
    parse_target, path_is_within, read_project_app_config, validate_output_name,
};
use derive_more::{Display, Error};

use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::{Config, Host};
use pnpm_engine_runtime_node_resolver::{
    get_node_mirror, parse_node_specifier, resolve_node_version,
};
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::parse_manifest;
use serde_json::Value;
use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

/// Minimum Node.js version that supports `node --build-sea`.
const MIN_BUILDER_VERSION: (u64, u64) = (25, 5);

/// Target OS names match Node's `process.platform`, keeping the CLI
/// surface consistent with pacquet's `--os` flag and
/// `supportedArchitectures.os` in `pnpm-workspace.yaml`.
const SUPPORTED_OS: &[&str] = &["linux", "darwin", "win32"];

const SUPPORTED_TARGETS: &str = "linux-x64, linux-x64-musl, linux-arm64, linux-arm64-musl, darwin-x64, darwin-arm64, win32-x64, win32-arm64";

/// `pacquet pack-app`: pack a CJS entry file into a standalone executable.
///
/// The executable embeds a Node.js binary via the Node.js Single
/// Executable Applications API. Requires the embedded runtime to be
/// Node.js v25.5+ (the minimum that supports `--build-sea`); a host-arch
/// Node.js of that version is downloaded to perform the injection.
///
/// Defaults for every flag can be set in `package.json` under `pnpm.app`.
/// CLI flags override the config; `--target` entirely replaces the
/// configured list so it can be narrowed at invocation time.
#[derive(Debug, Args)]
pub struct PackAppArgs {
    /// Positional arguments. The first is used as the CJS entry file when
    /// `--entry` is omitted; the rest are ignored.
    pub params: Vec<String>,

    /// Path to the CJS entry file to embed in the executable.
    #[clap(long)]
    pub entry: Option<String>,

    /// Target to build for. May be specified multiple times. Supported:
    /// linux-x64, linux-x64-musl, linux-arm64, linux-arm64-musl,
    /// darwin-x64, darwin-arm64, win32-x64, win32-arm64.
    #[clap(short = 't', long)]
    pub target: Vec<String>,

    /// Runtime to embed, as a `<name>@<version>` spec (e.g. `node@25`,
    /// `node@25.5.0`). Only `node` is supported, and the version must be
    /// >= v25.5. Defaults to the minimum SEA-capable version (v25.5.0).
    #[clap(long)]
    pub runtime: Option<String>,

    /// Output directory for the built executables. Defaults to `dist-app`.
    #[clap(short = 'o', long = "output-dir")]
    pub output_dir: Option<String>,

    /// Name for the output executable (without extension). Defaults to the
    /// unscoped package name.
    #[clap(long = "output-name")]
    pub output_name: Option<String>,
}

impl PackAppArgs {
    pub async fn run(self, config: &Config, dir: &Path) -> miette::Result<()> {
        // `pnpm.app` in package.json supplies defaults for every flag. CLI
        // flags win, but `--target` entirely replaces the config list
        // (additive merging would prevent narrowing from the CLI).
        let project = read_project_app_config(dir)?;

        let resolved_entry = dir.join(self.resolve_entry(&project, dir)?);

        let targets = self.resolve_targets(&project)?;

        // Parse the runtime before output-name derivation and any network
        // work so a malformed --runtime fails fast with a clear error
        // instead of being masked by later problems.
        let requested_node_spec = parse_runtime(&self.runtime_spec(&project))?;

        // Derive and validate the output name before creating any
        // directory, so an invalid `--output-name` / `pnpm.app.outputName`
        // (or a missing package name) fails fast without leaving an empty
        // `dist-app` behind.
        let output_name = self.resolve_output_name(&project, dir)?;

        let output_dir = self.resolve_output_dir(&project, dir)?;

        reject_non_regular_outputs(&targets, &output_dir, &output_name)?;

        let build_root = pnpm_home_dir()?.join("pack-app");

        // Resolve the embedded target version first so the builder can be
        // pinned to the same version. SEA blobs carry no version header and
        // the serialized format has changed across Node.js minor releases,
        // so a blob produced by a builder of a different version than the
        // embedded runtime fails deserialization at startup.
        let target_version = resolve_version(config, &requested_node_spec).await?;
        let build = SeaBuild {
            builder_bin: resolve_builder_binary(&build_root, &target_version)?,
            pacquet_bin: std::env::current_exe()
                .into_diagnostic()
                .wrap_err("resolving the pnpm executable path")?,
            build_root,
            target_version,
            dir,
            output_dir,
            output_name,
            entry: resolved_entry,
        };

        let results = targets
            .iter()
            .map(|target| build.build_target(target))
            .collect::<miette::Result<Vec<_>>>()?;
        print_built(&results);
        Ok(())
    }

    /// The runtime to embed: `--runtime`, else `pnpm.app.runtime`, else the
    /// default Node.js version.
    fn runtime_spec(&self, project: &ReadProjectAppConfigResult) -> String {
        self.runtime
            .clone()
            .or_else(|| {
                project.app
                    .as_ref()
                    .and_then(|app| app.runtime.clone())
            })
            .unwrap_or_else(|| format!("node@{}", default_runtime_version()))
    }

    /// The validated output name: `--output-name`, else
    /// `pnpm.app.outputName`, else one derived from the package name.
    fn resolve_output_name(
        &self,
        project: &ReadProjectAppConfigResult,
        dir: &Path,
    ) -> miette::Result<String> {
        let configured = self.output_name
            .clone()
            .or_else(|| {
                project.app
                    .as_ref()
                    .and_then(|app| app.output_name.clone())
            });
        let output_name = match configured {
            Some(name) => name,
            None => derive_output_name_from_package(project, dir)?,
        };
        Ok(validate_output_name(&output_name)?)
    }
    /// The created output directory. `outputDir` is repo-controllable, so
    /// absolute paths and `..` traversal are rejected — build artifacts
    /// must not be written outside the project.
    fn resolve_output_dir(
        &self,
        project: &ReadProjectAppConfigResult,
        dir: &Path,
    ) -> miette::Result<PathBuf> {
        let output_dir_raw = self.output_dir
            .clone()
            .or_else(|| {
                project.app
                    .as_ref()
                    .and_then(|app| app.output_dir.clone())
            })
            .unwrap_or_else(|| "dist-app".to_string());
        if escapes_project(&output_dir_raw) {
            return Err(PackAppError::OutputDirOutsideProject {
                path: output_dir_raw,
            }
            .into());
        }
        let output_dir = dir.join(&output_dir_raw);
        fs::create_dir_all(&output_dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("creating output directory {}", output_dir.display()))?;
        // Defense in depth against a symlinked `dist-app` (or configured
        // dir) that points out of the project: the lexical check above
        // can't see through a symlink, so re-check containment once the
        // real path exists.
        if !path_is_within(&output_dir, dir) {
            return Err(PackAppError::OutputDirOutsideProject {
                path: output_dir_raw,
            }
            .into());
        }
        Ok(output_dir)
    }

    /// The targets to build. `--target` replaces the configured list
    /// entirely rather than adding to it, so the CLI can narrow it.
    fn resolve_targets(
        &self,
        project: &ReadProjectAppConfigResult,
    ) -> miette::Result<Vec<ParsedTarget>> {
        let raw_targets: Vec<String> = if self.target.is_empty() {
            project.app
                .as_ref()
                .map(|app| app.targets.clone())
                .unwrap_or_default()
        } else {
            self.target.clone()
        };
        if raw_targets.is_empty() {
            return Err(PackAppError::MissingTarget {
                supported: SUPPORTED_TARGETS,
            }
            .into());
        }
        Ok(raw_targets
            .iter()
            .map(|raw| parse_target(raw))
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// The entry file, which `pnpm.app` may supply and the CLI overrides.
    ///
    /// The entry may come from a repo-controlled `package.json`, so
    /// absolute paths and `..` traversal are rejected before the
    /// filesystem is touched: the entry's *contents* get embedded into
    /// the produced executable, so an escaping path could exfiltrate a
    /// host file (an SSH key, say) into a distributable binary.
    fn resolve_entry(
        &self,
        project: &ReadProjectAppConfigResult,
        dir: &Path,
    ) -> miette::Result<String> {
        let entry_path = self.entry_path(project)?;
        if escapes_project(&entry_path) {
            return Err(PackAppError::EntryOutsideProject {
                path: entry_path,
            }
            .into());
        }
        let resolved_entry = dir.join(&entry_path);
        let entry_meta = fs::metadata(&resolved_entry)
            .map_err(|_| PackAppError::EntryNotFound {
                path: resolved_entry.display().to_string(),
            })?;
        if !entry_meta.is_file() {
            return Err(PackAppError::EntryNotFile {
                path: resolved_entry.display().to_string(),
            }
            .into());
        }
        // Defense in depth against a same-name symlink that points out of
        // the project: resolve symlinks and require the real path to stay
        // within the (also symlink-resolved) project directory.
        if !path_is_within(&resolved_entry, dir) {
            return Err(PackAppError::EntryOutsideProject {
                path: entry_path,
            }
            .into());
        }
        Ok(entry_path)
    }
}

impl SeaBuild<'_> {
    /// Build one target's executable and describe it for the summary.
    fn build_target(&self, target: &ParsedTarget) -> miette::Result<String> {
        let embedded_node_bin = ensure_node_runtime(
            &self.pacquet_bin,
            &self.build_root,
            &self.target_version,
            &target.platform,
            &target.arch,
            target.libc.as_deref(),
        )?;

        let target_output_dir = self.output_dir.join(&target.raw);
        fs::create_dir_all(&target_output_dir)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "creating target output directory {}",
                    target_output_dir.display(),
                )
            })?;
        // A repo could symlink `dist-app/<target>` out of the project even
        // when `dist-app` itself is contained; re-check the real path
        // before any binary is written into it.
        if !path_is_within(&target_output_dir, self.dir) {
            return Err(PackAppError::OutputDirOutsideProject {
                path: target_output_dir.display().to_string(),
            }
            .into());
        }

        let output_file =
            target_output_dir.join(output_file_name(&self.output_name, &target.platform));
        // Re-check the leaf path right before the build in case it became
        // a symlink after the upfront pass.
        reject_non_regular_output_file(&output_file)?;

        self.build_sea(&output_file, &embedded_node_bin)?;

        ad_hoc_sign_mac_binary(target, &output_file, self.dir)?;
        Ok(format!(
            "  {}: {} (Node.js {})",
            target.raw,
            output_file.display(),
            self.target_version,
        ))
    }
    fn build_sea(&self, output_file: &Path, embedded_node_bin: &Path) -> miette::Result<()> {
        let sea_config = serde_json::json!({
            "main": self.entry,
            "output": output_file,
            "executable": embedded_node_bin,
            "disableExperimentalSEAWarning": true,
            "useCodeCache": false,
            "useSnapshot": false,
        });
        // Write the SEA config into a fresh, unpredictable temp
        // directory (0700 by default) rather than a predictable path
        // under the system temp dir. Avoids TOCTOU/symlink attacks on
        // multi-user systems.
        let tmp_config_dir = tempfile::Builder::new()
            .prefix("pacquet-pack-app-")
            .tempdir()
            .into_diagnostic()
            .wrap_err("creating a temp directory for the SEA config")?;
        let config_path = tmp_config_dir.path().join("sea-config.json");
        fs::write(
            &config_path,
            serde_json::to_vec_pretty(&sea_config).expect("serialize SEA config"),
        )
        .into_diagnostic()
        .wrap_err("writing the SEA config")?;

        run_command(
            Command::new(&self.builder_bin).arg("--build-sea").arg(&config_path),
            "node --build-sea",
        )?;
        drop(tmp_config_dir);

        Ok(())
    }
}

#[cfg(test)]
mod tests;

mod config;

mod build;

mod errors;

impl PackAppArgs {
    fn entry_path(&self, project: &ReadProjectAppConfigResult) -> miette::Result<String> {
        let entry_path = self.entry
            .clone()
            .or_else(|| self.params.first().cloned())
            .or_else(|| {
                project.app
                    .as_ref()
                    .and_then(|app| app.entry.clone())
            })
            .ok_or(PackAppError::MissingEntry)?;
        Ok(entry_path)
    }
}
