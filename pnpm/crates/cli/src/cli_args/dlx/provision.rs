use super::{
    Config, DlxProgram, DlxSpawn, PackageManager, Path, PathBuf, Reporter, is_runtime_alias,
    is_version_request, materialize_runtime, provision, run_bin, split_spec,
};

/// A tool pnpm provisions itself rather than installing from the
/// registry.
pub(super) enum ProvisionedTool<'a> {
    PackageManager {
        pm: PackageManager,
        version_spec: &'a str,
        /// The specifier that named it, for the provisioning message.
        spec: &'a str,
        /// Which of the manager's bins to run, when `--package` named the
        /// manager and the command named the bin.
        bin: Option<&'a str>,
    },
    Runtime {
        name: &'a str,
        version_spec: &'a str,
    },
}

/// Which provisioned tool the command names, if any. `None` sends the
/// command down the ordinary registry-install path.
pub(super) fn provisioned_tool<'a>(
    package: &'a [String],
    bin_command: &'a str,
) -> Option<ProvisionedTool<'a>> {
    // An explicit `--package` is the user naming what to install, so it
    // stays literal; a bare tool name is provisioned instead of fetched
    // (see `engine_pm::selector` for why the name alone is not enough to
    // install one).
    if package.is_empty() {
        if let Some((pm, version_spec)) = parse_package_manager_spec(bin_command) {
            return Some(ProvisionedTool::PackageManager {
                pm,
                version_spec,
                spec: bin_command,
                bin: None,
            });
        }
        if let Some((name, version_spec)) = parse_runtime_spec(bin_command) {
            return Some(ProvisionedTool::Runtime { name, version_spec });
        }
    }

    // A package manager publishes more than one command, so naming it
    // with `--package` says which engine to provision while the command
    // says which of its bins to run: `pnx --package npm@11 npx`.
    let [spec] = package else { return None };
    let (pm, version_spec) = parse_package_manager_spec(spec)?;
    pm.bins().contains(&bin_command).then_some(ProvisionedTool::PackageManager {
        pm,
        version_spec,
        spec,
        bin: Some(bin_command),
    })
}

/// Split a dlx command word into the package manager it names and the
/// version specifier it asks for, or `None` when it names something else.
///
/// The bare name is the package manager's own line — `pnx yarn` is
/// whatever Yarn currently ships — matching how a `dlx` package spec
/// without a version resolves.
pub(super) fn parse_package_manager_spec(command: &str) -> Option<(PackageManager, &str)> {
    let (name, version_spec) = split_spec(command);
    let version_spec = version_spec.unwrap_or("latest");
    // A specifier that locates a package names what to install, not a line
    // of the tool, so it stays on the ordinary dlx path.
    if !is_version_request(version_spec) {
        return None;
    }
    Some((PackageManager::parse(name)?, version_spec))
}

/// Split a dlx command word into the runtime it names and the version
/// specifier it asks for, or `None` when it names something else.
pub(super) fn parse_runtime_spec(command: &str) -> Option<(&str, &str)> {
    let (name, version_spec) = split_spec(command);
    let version_spec = version_spec.unwrap_or("latest");
    // `node@runtime:22` is the same request spelled with the protocol the
    // resolver uses; anything else that locates a package is an ordinary
    // one.
    let version_spec = match version_spec.strip_prefix("runtime:") {
        Some(version_spec) => version_spec,
        None if is_version_request(version_spec) => version_spec,
        None => return None,
    };
    is_runtime_alias(name).then_some((name, version_spec))
}

/// Materialize `name` at `version_spec` and run it, the runtime half of
/// [`run_package_manager`].
pub(super) async fn run_runtime(
    state_dir: &Path,
    name: &str,
    version_spec: &str,
    command: &str,
    args: &[String],
    spawn: &DlxSpawn<'_>,
) -> miette::Result<()> {
    let executable =
        Box::pin(materialize_runtime(state_dir, name.to_string(), version_spec.to_string()))
            .await?;
    let bin_dirs: Vec<PathBuf> = executable.parent().map(Path::to_path_buf).into_iter().collect();
    run_bin(DlxProgram::Provisioned { command, executable: &executable }, args, bin_dirs, spawn)
}

/// Provision `pm` and run `bin` — or the engine's own command, when the
/// caller named none — with the caller's arguments, propagating the exit
/// status the way the rest of dlx does.
pub(super) async fn run_package_manager<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    pm: PackageManager,
    version_spec: &str,
    command: &str,
    bin: Option<&str>,
    args: &[String],
    spawn: &DlxSpawn<'_>,
) -> miette::Result<()> {
    let engine = Box::pin(provision::<Reporter>(config, pm, version_spec)).await?;
    let executable = match bin {
        Some(bin) => engine.command(bin),
        None => engine.program.clone(),
    };
    run_bin(
        DlxProgram::Provisioned { command, executable: &executable },
        args,
        engine.bin_dirs,
        spawn,
    )
}
