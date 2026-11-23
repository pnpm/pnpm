#![allow(clippy::disallowed_methods, reason = "a development tool runs its own commands")]
use crate::cli_args::{
    CliArgs,
    Lto,
    workspace_root,
};
use command_extra::CommandExtra;
use std::{
    path::{
        Path,
        PathBuf,
    },
    process::Command,
};

const PROBE_PACKAGE: &str = "pnpm-engine-size-probe";
const PROBE_BIN: &str = "engine-size-probe";

/// Where [`build_probe`] builds the probe and puts its copies.
pub struct BuildSite<'a> {
    /// Absolute, so cargo and this tool resolve it the same way.
    pub target_dir: &'a Path,
    /// The target triple, passed to cargo explicitly so a configured
    /// `build.target` cannot move the output directory.
    pub target: &'a str,
    pub out_dir: &'a Path,
}

/// The probe as built, and a copy with its symbols stripped.
pub struct ProbeBinaries {
    pub unstripped: PathBuf,
    pub stripped: PathBuf,
}

/// Build the probe with the workspace's `release` profile and `lto`.
///
/// The build keeps the symbols, which the attribution reads. The stripped
/// size is measured on a copy run through `strip_program`.
pub fn build_probe(
    args: &CliArgs,
    lto: Lto,
    site: &BuildSite<'_>,
) -> Result<ProbeBinaries, String> {
    let BuildSite { target_dir, target, out_dir } = *site;
    eprintln!("info: building {PROBE_BIN} with lto = {}", lto.as_str());
    let command = cargo()
        .with_current_dir(workspace_root())
        .with_args(["build", "--locked", "--release"]);
    let command = if args.report.no_default_features {
        command.with_arg("--no-default-features")
    } else {
        command
    };
    let status = command
        .with_args(["--package", PROBE_PACKAGE, "--target", target])
        .with_arg("--target-dir")
        .with_arg(target_dir)
        .with_env("CARGO_PROFILE_RELEASE_LTO", lto.as_str())
        .with_env("CARGO_PROFILE_RELEASE_STRIP", "false")
        .with_env("CARGO_PROFILE_RELEASE_DEBUG", "false")
        .status()
        .map_err(|error| format!("run cargo: {error}"))?;
    if !status.success() {
        return Err(format!("cargo build failed: {status}"));
    }
    let built = target_dir
        .join(target)
        .join("release")
        .join(executable_name(PROBE_BIN));
    let unstripped = out_dir.join(executable_name(&format!("{PROBE_BIN}-{}", lto.as_str())));
    let stripped = out_dir.join(executable_name(&format!("{PROBE_BIN}-{}-stripped", lto.as_str())));
    copy(&built, &unstripped)?;
    copy(&built, &stripped)?;
    strip(&args.strip_program, &stripped)?;
    Ok(ProbeBinaries { unstripped, stripped })
}

/// The target triple the probe is built for, from `rustc -vV`.
pub fn host_target() -> Result<String, String> {
    let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .with_current_dir(workspace_root())
        .with_arg("-vV")
        .output()
        .map_err(|error| format!("run rustc: {error}"))?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_string)
        .ok_or_else(|| "rustc -vV printed no host triple".to_string())
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

fn strip(program: &Path, binary: &Path) -> Result<(), String> {
    let status = Command::new(program)
        .with_arg(binary)
        .status()
        .map_err(|error| format!("run {}: {error}", program.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} {} failed: {status}", program.display(), binary.display()))
    }
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(drop)
        .map_err(|error| format!("copy {} to {}: {error}", from.display(), to.display()))
}

fn executable_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}
