//! Pacquet port of pnpm's
//! [`pack-app` command](https://github.com/pnpm/pnpm/blob/9f3df6b9b4/pnpm11/releasing/commands/src/pack-app/packApp.ts).
//!
//! Packs a `CommonJS` entry file into a standalone executable for one or
//! more target platforms, embedding a Node.js binary through the Node.js
//! [Single Executable Applications API](https://nodejs.org/api/single-executable-applications.html).
//!
//! Two faithful divergences from the pnpm source, both forced by pacquet
//! being a Rust binary rather than a Node.js script:
//!
//! - **The SEA builder is always downloaded.** pnpm reuses its own
//!   running interpreter (`process.execPath`) when it already matches the
//!   embedded runtime version. pacquet has no host Node.js to reuse, so it
//!   always fetches a host-arch Node.js of the embedded runtime version to
//!   run `--build-sea`.
//! - **The runtime install spawns the pacquet binary.** pnpm shells out to
//!   its own CLI via `runPnpmCli`; pacquet re-invokes itself
//!   (`std::env::current_exe()`) running `add node@runtime:<version>` with
//!   the target `--os` / `--cpu` / `--libc` flags into an isolated install
//!   directory under the pnpm home.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::{Config, Host};
use pacquet_engine_runtime_node_resolver::{
    get_node_mirror, parse_node_specifier, resolve_node_version,
};
use pacquet_network::{NetworkSettings, ThrottledClient};
use serde_json::Value;

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

/// Errors raised by `pacquet pack-app`.
///
/// The codes mirror pnpm's `PnpmError('PACK_APP_*', …)` (which prepends
/// `ERR_PNPM_`) so log consumers parse identical strings.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PackAppError {
    #[display(
        r#""pacquet pack-app" requires a CJS entry file — pass --entry <path> or set "pnpm.app.entry" in package.json."#
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_MISSING_ENTRY))]
    MissingEntry,

    #[display("Entry file not found: {path}")]
    #[diagnostic(code(ERR_PNPM_PACK_APP_ENTRY_NOT_FOUND))]
    EntryNotFound {
        #[error(not(source))]
        path: String,
    },

    #[display("Entry path must be a regular file: {path}")]
    #[diagnostic(code(ERR_PNPM_PACK_APP_ENTRY_NOT_FILE))]
    EntryNotFile {
        #[error(not(source))]
        path: String,
    },

    #[display(
        r#""pacquet pack-app" requires at least one target — pass --target <triplet> or set "pnpm.app.targets" in package.json. Supported: {supported}"#
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_MISSING_TARGET))]
    MissingTarget {
        #[error(not(source))]
        supported: &'static str,
    },

    #[display(
        r#"Invalid target: "{raw}". Expected format: <os>-<arch>[-<libc>] where <os> is {supported_os}, <arch> is x64|arm64, optional <libc> is musl (linux only)."#
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_TARGET))]
    InvalidTarget { raw: String, supported_os: String },

    #[display(r#"The "musl" libc suffix is only valid for linux targets (got "{raw}")."#)]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_TARGET))]
    MuslOnNonLinux {
        #[error(not(source))]
        raw: String,
    },

    #[display(
        r#"Invalid runtime "{spec}". Expected format: <name>@<version> (supported runtimes: node; e.g. "node@25.5.0")."#
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_RUNTIME))]
    InvalidRuntime {
        #[error(not(source))]
        spec: String,
    },

    #[display(
        r#"Invalid --output-name "{name}". The name must be a plain filename without path separators, Windows-reserved names (e.g. CON, NUL), characters like <>:"|?* or NUL, and must not end in a dot or space."#
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_OUTPUT_NAME))]
    InvalidOutputName {
        #[error(not(source))]
        name: String,
    },

    #[display("Unknown \"pnpm.app.{key}\" setting in package.json. Allowed keys: {allowed}.")]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_CONFIG))]
    UnknownConfigKey { key: String, allowed: String },

    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_CONFIG))]
    InvalidConfig {
        #[error(not(source))]
        message: String,
    },

    #[display("Failed to parse {path}: {message}")]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_PACKAGE_JSON))]
    InvalidPackageJson { path: String, message: String },

    #[display(r#"Could not determine the output name: package.json in {dir} has no "name" field."#)]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_NO_OUTPUT_NAME),
        help(r#"Pass --output-name <name> or set "pnpm.app.outputName" in package.json."#)
    )]
    NoOutputName {
        #[error(not(source))]
        dir: String,
    },

    #[display(
        "The embedded runtime \"node@{version}\" is older than Node.js v{major}.{minor}, which is the minimum version that supports --build-sea."
    )]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_RUNTIME_TOO_OLD),
        help(
            r#"Pass --runtime node@25.5.0 (or newer) or set "pnpm.app.runtime" in package.json."#
        )
    )]
    RuntimeTooOld { version: String, major: u64, minor: u64 },

    #[display(r#"Could not find a Node.js version that satisfies "{specifier}""#)]
    #[diagnostic(code(ERR_PNPM_PACK_APP_NODE_VERSION_NOT_FOUND))]
    NodeVersionNotFound {
        #[error(not(source))]
        specifier: String,
    },

    #[display(
        "Expected Node.js binary at {path} after installing node@runtime:{version}, but it was not found."
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_NODE_BINARY_MISSING))]
    NodeBinaryMissing { path: String, version: String },

    #[display("Cross-compiled macOS binary at {path} could not be ad-hoc signed with \"ldid\".")]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_MACOS_SIGN_FAILED),
        help(
            r#"Install ldid (https://github.com/ProcursusTeam/ldid) or re-sign the binary on macOS with "codesign --sign - <file>"."#
        )
    )]
    MacosSignFailed {
        #[error(not(source))]
        path: String,
    },

    #[display("Cannot ad-hoc sign the macOS binary at {path} on a {host} host.")]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_MACOS_SIGN_UNSUPPORTED_HOST),
        help(
            r#"Build macOS targets on a macOS or Linux host, or re-sign the produced binary yourself with "codesign --sign -" on macOS."#
        )
    )]
    MacosSignUnsupportedHost { path: String, host: String },
}

/// A parsed `<os>-<arch>[-<libc>]` target triplet.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTarget {
    raw: String,
    platform: String,
    arch: String,
    libc: Option<String>,
}

impl PackAppArgs {
    pub async fn run(self, config: &Config, dir: &Path) -> miette::Result<()> {
        // `pnpm.app` in package.json supplies defaults for every flag. CLI
        // flags win, but `--target` entirely replaces the config list
        // (additive merging would prevent narrowing from the CLI).
        let project = read_project_app_config(dir)?;

        let entry_path = self
            .entry
            .clone()
            .or_else(|| self.params.first().cloned())
            .or_else(|| project.app.as_ref().and_then(|app| app.entry.clone()))
            .ok_or(PackAppError::MissingEntry)?;
        let resolved_entry = dir.join(&entry_path);
        let entry_meta = fs::metadata(&resolved_entry).map_err(|_| {
            PackAppError::EntryNotFound { path: resolved_entry.display().to_string() }
        })?;
        if !entry_meta.is_file() {
            return Err(
                PackAppError::EntryNotFile { path: resolved_entry.display().to_string() }.into()
            );
        }

        let raw_targets: Vec<String> = if self.target.is_empty() {
            project.app.as_ref().map(|app| app.targets.clone()).unwrap_or_default()
        } else {
            self.target.clone()
        };
        if raw_targets.is_empty() {
            return Err(PackAppError::MissingTarget { supported: SUPPORTED_TARGETS }.into());
        }
        let targets =
            raw_targets.iter().map(|raw| parse_target(raw)).collect::<Result<Vec<_>, _>>()?;

        // Parse the runtime before output-name derivation and any network
        // work so a malformed --runtime fails fast with a clear error
        // instead of being masked by later problems.
        let runtime_spec = self
            .runtime
            .clone()
            .or_else(|| project.app.as_ref().and_then(|app| app.runtime.clone()))
            .unwrap_or_else(|| format!("node@{}", default_runtime_version()));
        let requested_node_spec = parse_runtime(&runtime_spec)?;

        // Derive and validate the output name before creating any
        // directory, so an invalid `--output-name` / `pnpm.app.outputName`
        // (or a missing package name) fails fast without leaving an empty
        // `dist-app` behind.
        let configured_output_name = self
            .output_name
            .clone()
            .or_else(|| project.app.as_ref().and_then(|app| app.output_name.clone()));
        let output_name = match configured_output_name {
            Some(name) => name,
            None => derive_output_name_from_package(&project, dir)?,
        };
        let output_name = validate_output_name(&output_name)?;

        let output_dir = dir.join(
            self.output_dir
                .clone()
                .or_else(|| project.app.as_ref().and_then(|app| app.output_dir.clone()))
                .unwrap_or_else(|| "dist-app".to_string()),
        );
        fs::create_dir_all(&output_dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("creating output directory {}", output_dir.display()))?;

        let build_root = pnpm_home_dir()?.join("pack-app");

        // Resolve the embedded target version first so the builder can be
        // pinned to the same version. SEA blobs carry no version header and
        // the serialized format has changed across Node.js minor releases,
        // so a blob produced by a builder of a different version than the
        // embedded runtime fails deserialization at startup.
        let resolved_target_version = resolve_version(config, &requested_node_spec).await?;
        let builder_bin = resolve_builder_binary(&build_root, &resolved_target_version)?;

        let pacquet_bin = std::env::current_exe()
            .into_diagnostic()
            .wrap_err("resolving the pacquet executable path")?;

        let mut results = Vec::with_capacity(targets.len());
        for target in &targets {
            let embedded_node_bin = ensure_node_runtime(
                &pacquet_bin,
                &build_root,
                &resolved_target_version,
                &target.platform,
                &target.arch,
                target.libc.as_deref(),
            )?;

            let target_output_dir = output_dir.join(&target.raw);
            fs::create_dir_all(&target_output_dir).into_diagnostic().wrap_err_with(|| {
                format!("creating target output directory {}", target_output_dir.display())
            })?;

            let output_file = if target.platform == "win32" {
                target_output_dir.join(format!("{output_name}.exe"))
            } else {
                target_output_dir.join(&output_name)
            };

            let sea_config = serde_json::json!({
                "main": resolved_entry,
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
                Command::new(&builder_bin).arg("--build-sea").arg(&config_path),
                "node --build-sea",
            )?;
            drop(tmp_config_dir);

            ad_hoc_sign_mac_binary(target, &output_file)?;

            results.push(format!(
                "  {}: {} (Node.js {resolved_target_version})",
                target.raw,
                output_file.display(),
            ));
        }

        let count = targets.len();
        let plural = if count == 1 { "" } else { "s" };
        println!("Built {count} executable{plural}:\n{}", results.join("\n"));
        Ok(())
    }
}

/// The Node.js version pack-app embeds when neither `--runtime` nor
/// `pnpm.app.runtime` is set.
///
/// pnpm defaults to its own running interpreter's version
/// (`process.version`). pacquet has no embedded Node.js, so it falls back
/// to the minimum SEA-capable version, which the embedded-runtime check
/// then accepts.
fn default_runtime_version() -> String {
    format!("{}.{}.0", MIN_BUILDER_VERSION.0, MIN_BUILDER_VERSION.1)
}

/// Returns a Node.js binary that supports `--build-sea` and produces a SEA
/// blob the embedded runtime can deserialize. The second constraint forces
/// the builder to match the target runtime version exactly.
///
/// Unlike pnpm — which reuses its own running interpreter when it already
/// matches — pacquet has no host Node.js, so it always downloads a
/// host-arch Node.js of the target version.
fn resolve_builder_binary(build_root: &Path, target_version: &str) -> miette::Result<PathBuf> {
    if !builder_version_can_build_sea(target_version) {
        return Err(PackAppError::RuntimeTooOld {
            version: target_version.to_string(),
            major: MIN_BUILDER_VERSION.0,
            minor: MIN_BUILDER_VERSION.1,
        }
        .into());
    }
    let pacquet_bin = std::env::current_exe()
        .into_diagnostic()
        .wrap_err("resolving the pacquet executable path")?;
    ensure_node_runtime(
        &pacquet_bin,
        build_root,
        target_version,
        pacquet_detect_libc::host_platform(),
        pacquet_detect_libc::host_arch(),
        // Pin libc to the host's. Otherwise a caller that set
        // supportedArchitectures.libc=musl in their config would cause the
        // glibc host to download a musl Node that it cannot execute.
        host_linux_libc(),
    )
}

fn host_linux_libc() -> Option<&'static str> {
    if pacquet_detect_libc::host_platform() != "linux" {
        return None;
    }
    Some(pacquet_detect_libc::detect().map_or("glibc", |impl_| impl_.as_str()))
}

fn builder_version_can_build_sea(version: &str) -> bool {
    let mut parts = version.split('.');
    let major = parts.next().and_then(|major| major.parse::<u64>().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|minor| minor.parse::<u64>().ok()).unwrap_or(0);
    major > MIN_BUILDER_VERSION.0
        || (major == MIN_BUILDER_VERSION.0 && minor >= MIN_BUILDER_VERSION.1)
}

/// Fetches a Node.js runtime into a dedicated per-target directory under
/// the pnpm home, reusing the cached binary if already present. Actual
/// files are hardlinked from pacquet's content-addressable store, so
/// repeated calls are cheap.
///
/// Mirrors pnpm's `runPnpmCli(['add', …])` by re-invoking the pacquet
/// binary against an isolated install directory.
fn ensure_node_runtime(
    pacquet_bin: &Path,
    build_root: &Path,
    version: &str,
    platform: &str,
    arch: &str,
    libc: Option<&str>,
) -> miette::Result<PathBuf> {
    // Linux variants always need a libc pin (glibc or musl) so variant
    // selection is deterministic and doesn't depend on the host's detected
    // libc or the user's supportedArchitectures.libc config.
    let libc = if platform == "linux" { Some(libc.unwrap_or("glibc")) } else { libc };
    let target_id =
        [Some(platform), Some(arch), libc].into_iter().flatten().collect::<Vec<_>>().join("-");
    let install_dir = build_root.join(format!("{target_id}-{version}"));
    let node_dir = install_dir.join("node_modules").join("node");
    let binary_path = node_binary_path(&node_dir, platform);
    if binary_path.exists() {
        return Ok(binary_path);
    }

    fs::create_dir_all(&install_dir).into_diagnostic().wrap_err_with(|| {
        format!("creating the runtime install directory {}", install_dir.display())
    })?;
    fs::write(
        install_dir.join("package.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": format!("pacquet-pack-app-{target_id}"),
                "private": true,
            }))
            .expect("serialize the runtime install manifest"),
        ),
    )
    .into_diagnostic()
    .wrap_err("writing the runtime install manifest")?;

    let mut command = Command::new(pacquet_bin);
    command
        .arg("-C")
        .arg(&install_dir)
        .arg("add")
        .arg(format!("--os={platform}"))
        .arg(format!("--cpu={arch}"));
    if let Some(libc) = libc {
        command.arg(format!("--libc={libc}"));
    }
    command.arg(format!("node@runtime:{version}"));
    run_command(&mut command, "pacquet add node@runtime")?;

    if !binary_path.exists() {
        return Err(PackAppError::NodeBinaryMissing {
            path: binary_path.display().to_string(),
            version: version.to_string(),
        }
        .into());
    }
    Ok(binary_path)
}

fn node_binary_path(node_dir: &Path, platform: &str) -> PathBuf {
    if platform == "win32" { node_dir.join("node.exe") } else { node_dir.join("bin").join("node") }
}

async fn resolve_version(config: &Config, specifier: &str) -> miette::Result<String> {
    let parsed = parse_node_specifier(specifier).map_err(miette::Report::new)?;
    // pacquet has no `node-download-mirrors` config field yet, so the
    // override map is always absent and the official nodejs.org tree is
    // used. Matches pnpm's default when `nodeDownloadMirrors` is unset.
    let mirror = get_node_mirror(None, &parsed.release_channel);
    let http_client = build_http_client(config)?;
    let version = resolve_node_version(&http_client, &parsed.version_specifier, Some(&mirror))
        .await
        .map_err(miette::Report::new)?;
    // The resolved version becomes a path component of the per-target
    // runtime cache dir (`<build_root>/<target_id>-<version>`). For the
    // `latest` / channel selectors the resolver returns the mirror's first
    // `index.json` entry without semver validation, so a compromised mirror
    // could smuggle `..` or a path separator and escape the cache dir.
    // Require a parseable semver before the string is ever used as a path.
    let version = version.filter(|version| node_semver::Version::parse(version).is_ok());
    version.ok_or_else(|| {
        PackAppError::NodeVersionNotFound { specifier: specifier.to_string() }.into()
    })
}

/// The network client pack-app resolves Node.js versions through, built
/// from the same proxy / TLS / timeout config as the install client.
fn build_http_client(config: &Config) -> miette::Result<ThrottledClient> {
    ThrottledClient::for_installs(
        &config.proxy,
        &config.tls,
        &config.tls_by_uri,
        &NetworkSettings {
            network_concurrency: config.network_concurrency,
            fetch_timeout: Duration::from_millis(config.fetch_timeout),
            user_agent: config.user_agent.clone(),
        },
    )
    .into_diagnostic()
    .wrap_err("create the network client for pack-app")
}

fn parse_target(raw: &str) -> Result<ParsedTarget, PackAppError> {
    // Anchored, segment-constrained parse so inputs like
    // `linux-x64-musl-../../outside` are rejected outright — otherwise
    // `target.raw` would later flow into the output directory join and
    // could escape it.
    let parts: Vec<&str> = raw.split('-').collect();
    let invalid = || PackAppError::InvalidTarget {
        raw: raw.to_string(),
        supported_os: SUPPORTED_OS.join("|"),
    };
    let (platform, arch, libc) = match parts.as_slice() {
        [platform, arch] => (*platform, *arch, None),
        [platform, arch, libc] => (*platform, *arch, Some(*libc)),
        _ => return Err(invalid()),
    };
    if !SUPPORTED_OS.contains(&platform) {
        return Err(invalid());
    }
    if arch != "x64" && arch != "arm64" {
        return Err(invalid());
    }
    if let Some(libc) = libc {
        if libc != "musl" {
            return Err(invalid());
        }
        if platform != "linux" {
            return Err(PackAppError::MuslOnNonLinux { raw: raw.to_string() });
        }
    }
    Ok(ParsedTarget {
        raw: raw.to_string(),
        platform: platform.to_string(),
        arch: arch.to_string(),
        libc: libc.map(ToString::to_string),
    })
}

/// Runtime spec is `<name>@<version>`. Only `node` is supported today; the
/// prefix is kept so future runtimes (bun, deno) can share the flag
/// without a breaking change.
fn parse_runtime(spec: &str) -> Result<String, PackAppError> {
    let invalid = || PackAppError::InvalidRuntime { spec: spec.to_string() };
    let (name, version) = spec.split_once('@').ok_or_else(invalid)?;
    if name != "node" || version.is_empty() {
        return Err(invalid());
    }
    Ok(version.to_string())
}

/// Win32 reserved device names (case-insensitive, with or without an
/// extension).
fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

/// Reject anything that would let the output escape its target directory,
/// or that would fail filesystem-level validation on any supported host.
fn validate_output_name(name: &str) -> Result<String, PackAppError> {
    let basename = Path::new(name).file_name().and_then(|n| n.to_str());
    let invalid_chars =
        name.chars().any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\0'));
    let trailing_dot_or_space = name.ends_with('.') || name.ends_with(' ');
    if basename != Some(name)
        || name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || invalid_chars
        || is_reserved_windows_name(name)
        || trailing_dot_or_space
    {
        return Err(PackAppError::InvalidOutputName { name: name.to_string() });
    }
    Ok(name.to_string())
}

/// Fields pack-app reads from `pnpm.app` in package.json.
#[derive(Debug, Default)]
struct ProjectAppConfig {
    entry: Option<String>,
    targets: Vec<String>,
    runtime: Option<String>,
    output_dir: Option<String>,
    output_name: Option<String>,
}

#[derive(Debug, Default)]
struct ReadProjectAppConfigResult {
    name: Option<String>,
    app: Option<ProjectAppConfig>,
}

/// A narrow reader just for this command: a `package.json` with optional
/// `pnpm.app` settings, without the installable/engine checks the regular
/// manifest reader would impose.
fn read_project_app_config(dir: &Path) -> Result<ReadProjectAppConfigResult, PackAppError> {
    let manifest_path = dir.join("package.json");
    let Ok(raw) = fs::read_to_string(&manifest_path) else {
        return Ok(ReadProjectAppConfigResult::default());
    };
    let manifest: Value =
        serde_json::from_str(&raw).map_err(|err| PackAppError::InvalidPackageJson {
            path: manifest_path.display().to_string(),
            message: err.to_string(),
        })?;
    let Some(manifest) = manifest.as_object() else {
        return Ok(ReadProjectAppConfigResult::default());
    };
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string);
    let app_field = manifest
        .get("pnpm")
        .and_then(Value::as_object)
        .and_then(|pnpm| pnpm.get("app"))
        .and_then(Value::as_object);
    let Some(app_field) = app_field else {
        return Ok(ReadProjectAppConfigResult { name, app: None });
    };
    Ok(ReadProjectAppConfigResult { name, app: Some(validate_app_config(app_field)?) })
}

fn validate_app_config(
    raw: &serde_json::Map<String, Value>,
) -> Result<ProjectAppConfig, PackAppError> {
    const KNOWN: &[&str] = &["entry", "targets", "runtime", "outputDir", "outputName"];
    for key in raw.keys() {
        if !KNOWN.contains(&key.as_str()) {
            return Err(PackAppError::UnknownConfigKey {
                key: key.clone(),
                allowed: KNOWN.join(", "),
            });
        }
    }
    let string_field = |key: &str| -> Result<Option<String>, PackAppError> {
        match raw.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value.clone())),
            Some(_) => Err(PackAppError::InvalidConfig {
                message: format!("\"pnpm.app.{key}\" must be a string."),
            }),
        }
    };
    let targets = match raw.get("targets") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().map(ToString::to_string).ok_or_else(|| PackAppError::InvalidConfig {
                    message: r#""pnpm.app.targets" must be an array of strings."#.to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(PackAppError::InvalidConfig {
                message: r#""pnpm.app.targets" must be an array of strings."#.to_string(),
            });
        }
    };
    Ok(ProjectAppConfig {
        entry: string_field("entry")?,
        targets,
        runtime: string_field("runtime")?,
        output_dir: string_field("outputDir")?,
        output_name: string_field("outputName")?,
    })
}

fn derive_output_name_from_package(
    project: &ReadProjectAppConfigResult,
    dir: &Path,
) -> Result<String, PackAppError> {
    let Some(name) = project.name.as_deref() else {
        return Err(PackAppError::NoOutputName { dir: dir.display().to_string() });
    };
    // Strip the `@scope/` prefix from scoped packages so the binary name is
    // a plain filename. The downstream `validate_output_name` pass rejects
    // any leftover path separators.
    let unscoped = if let Some(rest) = name.strip_prefix('@') {
        rest.split_once('/').map_or(name, |(_, rest)| rest)
    } else {
        name
    };
    Ok(unscoped.to_string())
}

/// pnpm home directory, the base of pack-app's per-target runtime cache.
/// Mirrors pnpm's `config.pnpmHomeDir`.
fn pnpm_home_dir() -> miette::Result<PathBuf> {
    pacquet_config::default_pnpm_home_dir::<Host>()
        .ok_or_else(|| miette::miette!("could not determine the pnpm home directory"))
}

/// SEA injection invalidates the existing code signature on macOS
/// binaries, so the output must be re-signed. Native macOS hosts use
/// `codesign`; Linux hosts cross-signing a darwin target use `ldid`.
/// Windows hosts have no readily available ad-hoc signer.
fn ad_hoc_sign_mac_binary(target: &ParsedTarget, output_file: &Path) -> miette::Result<()> {
    if target.platform != "darwin" {
        return Ok(());
    }
    match pacquet_detect_libc::host_platform() {
        "darwin" => run_command(
            Command::new("codesign").arg("--sign").arg("-").arg(output_file),
            "codesign",
        ),
        "linux" => {
            run_command(Command::new("ldid").arg("-S").arg(output_file), "ldid").map_err(|_| {
                PackAppError::MacosSignFailed { path: output_file.display().to_string() }.into()
            })
        }
        host => Err(PackAppError::MacosSignUnsupportedHost {
            path: output_file.display().to_string(),
            host: host.to_string(),
        }
        .into()),
    }
}

/// Run a child process inheriting stdio, erroring on spawn failure or a
/// non-zero exit status.
fn run_command(command: &mut Command, label: &str) -> miette::Result<()> {
    let status = command.status().into_diagnostic().wrap_err_with(|| format!("running {label}"))?;
    if !status.success() {
        return Err(miette::miette!("{label} exited with {status}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
