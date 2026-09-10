use super::{
    Command, Config, Context, Host, IntoDiagnostic, MIN_BUILDER_VERSION, PackAppError,
    ParsedTarget, Path, PathBuf, ThrottledClient, fs, get_node_mirror, output_file_name,
    parse_node_specifier, path_is_within, resolve_node_version,
};

/// Everything one target's SEA build reads besides the target itself.
pub(super) struct SeaBuild<'a> {
    pub(super) dir: &'a Path,
    pub(super) output_dir: PathBuf,
    pub(super) output_name: String,
    pub(super) entry: PathBuf,
    pub(super) build_root: PathBuf,
    pub(super) target_version: String,
    pub(super) builder_bin: PathBuf,
    pub(super) pacquet_bin: PathBuf,
}

/// Reject a pre-existing symlink (or any non-regular file) at any
/// target's final output path before downloading anything: a repo
/// could commit `dist-app/<target>/<name>` as a symlink pointing
/// outside the project, and `node --build-sea` would follow it to
/// overwrite an arbitrary file. The directory containment checks
/// do not cover the leaf file.
pub(super) fn reject_non_regular_outputs(
    targets: &[ParsedTarget],
    output_dir: &Path,
    output_name: &str,
) -> Result<(), PackAppError> {
    for target in targets {
        let output_file =
            output_dir.join(&target.raw).join(output_file_name(output_name, &target.platform));
        reject_non_regular_output_file(&output_file)?;
    }
    Ok(())
}

pub(super) fn print_built(results: &[String]) {
    let count = results.len();
    let plural = if count == 1 { "" } else { "s" };
    println!("Built {count} executable{plural}:\n{}", results.join("\n"));
}

/// Returns a Node.js binary that supports `--build-sea` and produces a SEA
/// blob the embedded runtime can deserialize. The second constraint forces
/// the builder to match the target runtime version exactly.
///
/// Unlike pnpm — which reuses its own running interpreter when it already
/// matches — pacquet has no host Node.js, so it always downloads a
/// host-arch Node.js of the target version.
pub(super) fn resolve_builder_binary(
    build_root: &Path,
    target_version: &str,
) -> miette::Result<PathBuf> {
    if !builder_version_can_build_sea(target_version) {
        return Err(PackAppError::RuntimeTooOld {
            version: target_version.to_string(),
            major: MIN_BUILDER_VERSION.0,
            minor: MIN_BUILDER_VERSION.1,
        }
        .into());
    }
    let pacquet_bin =
        std::env::current_exe().into_diagnostic().wrap_err("resolving the pnpm executable path")?;
    ensure_node_runtime(
        &pacquet_bin,
        build_root,
        target_version,
        pnpm_detect_libc::host_platform(),
        pnpm_detect_libc::host_arch(),
        // Pin libc to the host's. Otherwise a caller that set
        // supportedArchitectures.libc=musl in their config would cause the
        // glibc host to download a musl Node that it cannot execute.
        host_linux_libc(),
    )
}

fn host_linux_libc() -> Option<&'static str> {
    if pnpm_detect_libc::host_platform() != "linux" {
        return None;
    }
    Some(pnpm_detect_libc::detect().map_or("glibc", |impl_| impl_.as_str()))
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
/// Re-invokes the pacquet binary with `add` against an isolated install
/// directory.
pub(super) fn ensure_node_runtime(
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

    write_runtime_install_manifest(&install_dir, &target_id)?;

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
    run_command(&mut command, "pnpm add node@runtime")?;

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

pub(super) async fn resolve_version(config: &Config, specifier: &str) -> miette::Result<String> {
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
        &config.network_settings(),
    )
    .into_diagnostic()
    .wrap_err("create the network client for pack-app")
}

/// Refuse to write to `output_file` when it already exists and is not a
/// regular file — most importantly a symlink, which `node --build-sea`
/// would follow to overwrite a file outside the project. `symlink_metadata`
/// does not traverse the final component, so a symlink reports
/// `is_file() == false`. A missing path is fine (nothing to overwrite).
pub(super) fn reject_non_regular_output_file(output_file: &Path) -> Result<(), PackAppError> {
    match fs::symlink_metadata(output_file) {
        Ok(meta) if !meta.is_file() => {
            Err(PackAppError::OutputFileNotRegular { path: output_file.display().to_string() })
        }
        _ => Ok(()),
    }
}

/// pnpm home directory, the base of pack-app's per-target runtime cache.
pub(super) fn pnpm_home_dir() -> miette::Result<PathBuf> {
    pnpm_config::default_pnpm_home_dir::<Host>()
        .ok_or_else(|| miette::miette!("could not determine the pnpm home directory"))
}

/// SEA injection invalidates the existing code signature on macOS
/// binaries, so the output must be re-signed. Native macOS hosts use
/// `codesign`; Linux hosts cross-signing a darwin target use `ldid`.
/// Windows hosts have no readily available ad-hoc signer.
pub(super) fn ad_hoc_sign_mac_binary(
    target: &ParsedTarget,
    output_file: &Path,
    dir: &Path,
) -> miette::Result<()> {
    if target.platform != "darwin" {
        return Ok(());
    }
    match pnpm_detect_libc::host_platform() {
        // `codesign` is a macOS system tool; spawn it by absolute path so a
        // repo-controlled `node_modules/.bin/codesign` on PATH can't be run
        // in its place.
        "darwin" => run_command(
            Command::new("/usr/bin/codesign").arg("--sign").arg("-").arg(output_file),
            "codesign",
        ),
        "linux" => {
            let ldid = resolve_trusted_signer("ldid", dir, output_file)?;
            run_command(Command::new(&ldid).arg("-S").arg(output_file), "ldid").map_err(|_| {
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

/// Resolve an external signer (`ldid`) to an absolute path via `PATH`,
/// skipping any match that resolves inside the project directory — a repo
/// could ship `node_modules/.bin/ldid` and, if that directory is on the
/// developer's `PATH`, get an attacker-controlled binary executed when
/// packaging a darwin target. Returns the first match outside the project,
/// or [`PackAppError::MacosSignFailed`] when none is found.
fn resolve_trusted_signer(
    name: &str,
    dir: &Path,
    output_file: &Path,
) -> Result<PathBuf, PackAppError> {
    let sign_failed = || PackAppError::MacosSignFailed { path: output_file.display().to_string() };
    let matches = which::which_all(name).map_err(|_| sign_failed())?;
    first_signer_outside_project(matches, dir).ok_or_else(sign_failed)
}

/// The first resolved signer path that is not inside the project directory.
/// Split out from [`resolve_trusted_signer`] so the skip-project-local rule is
/// unit-testable without manipulating the process `PATH`.
pub(super) fn first_signer_outside_project(
    mut matches: impl Iterator<Item = PathBuf>,
    dir: &Path,
) -> Option<PathBuf> {
    matches.find(|resolved| !path_is_within(resolved, dir))
}

/// Run a child process inheriting stdio, erroring on spawn failure or a
/// non-zero exit status.
pub(super) fn run_command(command: &mut Command, label: &str) -> miette::Result<()> {
    let status = command.status().into_diagnostic().wrap_err_with(|| format!("running {label}"))?;
    if !status.success() {
        return Err(miette::miette!("{label} exited with {status}"));
    }
    Ok(())
}

fn write_runtime_install_manifest(install_dir: &Path, target_id: &str) -> miette::Result<()> {
    fs::create_dir_all(install_dir).into_diagnostic().wrap_err_with(|| {
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

    Ok(())
}
