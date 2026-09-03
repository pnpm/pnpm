use super::{
    access::AccessArgs,
    add::AddArgs,
    approve_builds::ApproveBuildsArgs,
    audit::AuditArgs,
    bin::BinArgs,
    bugs::BugsArgs,
    cache::CacheCommand,
    cat_file::CatFileArgs,
    cat_index::CatIndexArgs,
    change::ChangeArgs,
    ci::CiArgs,
    clean::CleanArgs,
    completion::{CompletionArgs, CompletionServerArgs},
    config::{ConfigArgs, ConfigGetAliasArgs, ConfigSetAliasArgs},
    create::CreateArgs,
    dedupe::DedupeArgs,
    deploy::DeployArgs,
    deprecate::DeprecateArgs,
    dist_tag::DistTagArgs,
    dlx::DlxArgs,
    docs::DocsArgs,
    doctor::DoctorArgs,
    env::EnvArgs,
    exec::ExecArgs,
    fetch::FetchArgs,
    find_hash::FindHashArgs,
    ignored_builds::IgnoredBuildsArgs,
    import::ImportArgs,
    init::InitArgs,
    install::InstallArgs,
    install_test::InstallTestArgs,
    lane::LaneArgs,
    licenses::LicensesArgs,
    link::LinkArgs,
    list::ListArgs,
    login::LoginArgs,
    logout::LogoutArgs,
    not_implemented::NotImplementedArgs,
    outdated::OutdatedArgs,
    owner::OwnerArgs,
    pack::PackArgs,
    pack_app::PackAppArgs,
    patch::PatchArgs,
    patch_commit::PatchCommitArgs,
    patch_remove::PatchRemoveArgs,
    peers::PeersArgs,
    ping::PingArgs,
    pkg::PkgArgs,
    prefix::PrefixArgs,
    prune::PruneArgs,
    publish::PublishArgs,
    rebuild::RebuildArgs,
    remove::RemoveArgs,
    repo::RepoArgs,
    reporter::{LogLevelSetting, ReporterType},
    restart::RestartArgs,
    root::RootArgs,
    run::RunArgs,
    runtime::RuntimeArgs,
    sbom::SbomArgs,
    script_shortcut::ScriptShortcutArgs,
    search::SearchArgs,
    self_update::SelfUpdateArgs,
    set_script::SetScriptArgs,
    setup::SetupArgs,
    shim::ShimArgs,
    stage::StageArgs,
    star::StarArgs,
    stars::StarsArgs,
    store::StoreCommand,
    team::TeamArgs,
    undeprecate::UndeprecateArgs,
    unlink::UnlinkArgs,
    unpublish::UnpublishArgs,
    unstar::UnstarArgs,
    update::UpdateArgs,
    version::VersionArgs,
    view::ViewArgs,
    why::WhyArgs,
    with::WithArgs,
};
use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pipe_trait::Pipe;
use pnpm_default_reporter::SummaryScope;
use std::path::PathBuf;

/// Experimental package manager for node.js written in rust.
#[derive(Debug, Parser)]
#[clap(name = "pnpm")]
#[clap(bin_name = "pnpm")]
#[clap(version = pnpm_config::PNPM_VERSION)]
#[clap(disable_version_flag = true)]
#[clap(about = "Experimental package manager for node.js")]
pub struct CliArgs {
    #[clap(subcommand)]
    pub command: CliCommand,

    /// Print the pnpm version.
    // Replaces clap's built-in flag, whose short form is `-V` and whose
    // output is prefixed; pnpm uses `-v` and prints the bare version in
    // `main` when the `Version` action raises `DisplayVersion`.
    #[clap(short = 'v', long = "version", action = clap::ArgAction::Version)]
    pub version: Option<bool>,

    /// Force colored output.
    #[clap(
        long,
        global = true,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "always",
        value_parser = parse_color_mode,
        overrides_with = "no_color"
    )]
    pub color: Option<pnpm_config::ColorMode>,

    /// Disable colored output.
    #[clap(long = "no-color", global = true, hide = true, overrides_with = "color")]
    pub no_color: bool,

    /// Automatically answer yes to prompts.
    #[clap(short = 'y', long, global = true)]
    pub yes: bool,

    /// Set working directory. Accepted anywhere on the command line,
    /// before or after the subcommand, like every other rc-option.
    #[clap(short = 'C', long, alias = "prefix", default_value = ".", global = true)]
    pub dir: PathBuf,

    /// Directory in which the package store is created. Relative paths
    /// are resolved from the workspace root, or from `--dir` outside a
    /// workspace.
    #[clap(
        long = "store-dir",
        alias = "store",
        value_name = "DIR",
        global = true,
        overrides_with = "store_dir",
        value_parser = parse_store_dir
    )]
    pub store_dir: Option<PathBuf>,

    /// Directory in which pnpm persists machine-local state.
    #[clap(long = "state-dir", value_name = "DIR", global = true, overrides_with = "state_dir")]
    pub state_dir: Option<PathBuf>,

    /// Path to an `.npmrc` to read auth settings from, overriding the
    /// default `~/.npmrc`.
    #[clap(long = "npmrc-auth-file", visible_alias = "userconfig", global = true)]
    pub npmrc_auth_file: Option<PathBuf>,

    /// Base URL of the npm registry to resolve and fetch packages from.
    /// Universal rc-option: accepted on every command and layered onto
    /// the config like `--config.registry=<url>`. Commands that expose
    /// their own `--registry` still read the same value.
    #[clap(long, global = true)]
    pub registry: Option<String>,

    /// Proxy for HTTPS registry and tarball requests.
    #[clap(long = "https-proxy", global = true)]
    pub https_proxy: Option<String>,

    /// Proxy for HTTP registry and tarball requests.
    #[clap(long = "http-proxy", global = true)]
    pub http_proxy: Option<String>,

    /// Hosts that bypass configured proxies.
    #[clap(long = "no-proxy", global = true)]
    pub no_proxy: Option<String>,

    /// Run the command for every project in the workspace instead of only
    /// the project in `--dir`.
    #[clap(short = 'r', long, global = true)]
    pub recursive: bool,

    /// Reporter output format.
    // Self-override so a repeated `--reporter` takes the last occurrence,
    // like nopt does — `--silent` expands to `--reporter=silent` (see
    // `crate::shorthands`), so `--silent --reporter=ndjson` must not be a
    // duplicate-argument error.
    #[clap(
        long,
        value_enum,
        default_value_t = ReporterType::Default,
        global = true,
        overrides_with = "reporter"
    )]
    pub reporter: ReporterType,

    /// What level of logs to print. Mirrors pnpm's universal `--loglevel`
    /// option: `silent` selects the silent reporter over any `--reporter`
    /// choice; the other levels cap the default reporter's output.
    #[clap(long, value_enum, global = true)]
    pub loglevel: Option<LogLevelSetting>,

    /// Select which workspace projects to run on. Repeat to add more.
    /// Each selector can be a name pattern (`@scope/*`), a path (`./pkg`),
    /// a dependency query (`foo...`), an exclusion (`!bar`), a directory
    /// (`{dir}`), or a changed-since query (`[since]`).
    #[clap(short = 'F', long, global = true)]
    pub filter: Vec<String>,

    /// Like `--filter`, but follow only production dependencies when
    /// selecting projects.
    #[clap(long = "filter-prod", global = true)]
    pub filter_prod: Vec<String>,

    /// Run the command on the root workspace project.
    #[clap(short = 'w', long = "workspace-root", global = true)]
    pub workspace_root: bool,

    /// Exit with code 1 when the `--filter` / `--filter-prod` selectors
    /// match no workspace project.
    #[clap(long = "fail-if-no-match", global = true)]
    pub fail_if_no_match: bool,

    /// Also run a recursive command on the root workspace project, which
    /// `run` / `exec` / `add` / `test` otherwise leave out.
    #[clap(
        long = "include-workspace-root",
        global = true,
        overrides_with = "no_include_workspace_root"
    )]
    pub include_workspace_root: bool,

    /// Leave the root workspace project out of a recursive command,
    /// overriding an `includeWorkspaceRoot: true` setting.
    #[clap(
        long = "no-include-workspace-root",
        global = true,
        overrides_with = "include_workspace_root"
    )]
    pub no_include_workspace_root: bool,

    /// Glob patterns naming test files, used by the `[since]` `--filter`
    /// selector to decide which changes count.
    #[clap(long = "test-pattern", global = true)]
    pub test_pattern: Vec<String>,

    /// Glob patterns of changed files that the `[since]` `--filter`
    /// selector should ignore.
    #[clap(long = "changed-files-ignore-pattern", global = true)]
    pub changed_files_ignore_pattern: Vec<String>,

    /// Keep recursive workspace projects sorted topologically.
    #[clap(long = "sort", global = true, overrides_with = "no_sort")]
    pub sort: bool,

    /// Run recursive workspace projects in workspace order.
    #[clap(long = "no-sort", global = true, overrides_with = "sort")]
    pub no_sort: bool,

    /// Process recursive workspace projects in reverse order.
    #[clap(long, global = true, overrides_with = "no_reverse")]
    pub reverse: bool,

    /// Process recursive workspace projects in their normal order.
    #[clap(long = "no-reverse", global = true, hide = true, overrides_with = "reverse")]
    pub no_reverse: bool,

    /// Maximum number of workspace projects to process in parallel.
    #[clap(long = "workspace-concurrency", global = true)]
    pub workspace_concurrency: Option<i32>,

    /// Run scripts in every selected workspace project concurrently,
    /// disregarding topological sorting.
    #[clap(long, global = true)]
    pub parallel: bool,

    /// Recursive only: resume execution from the given package.
    #[clap(long = "resume-from", global = true, hide = true)]
    pub resume_from: Option<String>,

    /// Recursive only: write a `pnpm-exec-summary.json` execution report.
    #[clap(long = "report-summary", global = true, hide = true)]
    pub report_summary: bool,

    /// Recursive only: keep going after a project fails.
    #[clap(long = "no-bail", global = true, hide = true, overrides_with = "bail")]
    pub no_bail: bool,

    /// Stop a recursive command after the first failure.
    #[clap(long, global = true, hide = true, overrides_with = "no_bail")]
    pub bail: bool,

    /// Don't fail when the named script is undefined.
    #[clap(long = "if-present", hide = true)]
    pub if_present: bool,

    /// Stream a recursive command's script output as it arrives, one
    /// prefixed line at a time.
    #[clap(long, global = true)]
    pub stream: bool,

    /// Hold each script's streamed output until the script exits, then
    /// print it as one block.
    #[clap(long = "aggregate-output", global = true)]
    pub aggregate_output: bool,

    /// Divert the reporter's output to stderr, leaving stdout for the
    /// command's own result.
    #[clap(long = "use-stderr", global = true)]
    pub use_stderr: bool,

    /// Omit the project prefix from the streamed output of running
    /// scripts. A `run` / `exec` option pnpm accepts anywhere on the
    /// command line, like the recursive-run flags above.
    #[clap(
        long = "reporter-hide-prefix",
        global = true,
        hide = true,
        overrides_with = "no_reporter_hide_prefix"
    )]
    pub reporter_hide_prefix: bool,

    /// Prefix the streamed output of running scripts with the project
    /// it came from, overriding a `reporterHidePrefix: true` setting.
    #[clap(
        long = "no-reporter-hide-prefix",
        global = true,
        hide = true,
        overrides_with = "reporter_hide_prefix"
    )]
    pub no_reporter_hide_prefix: bool,

    /// Run as if the project were standalone, ignoring any
    /// `pnpm-workspace.yaml` above it.
    #[clap(long = "ignore-workspace", global = true)]
    pub ignore_workspace: bool,

    /// Glob patterns selecting the workspace's projects, overriding the
    /// `packages` field of `pnpm-workspace.yaml`. Repeat to add more.
    #[clap(long = "workspace-packages", global = true)]
    pub workspace_packages: Vec<String>,
}

fn parse_store_dir(value: &str) -> Result<PathBuf, std::convert::Infallible> {
    Ok(PathBuf::from(value))
}

fn parse_color_mode(value: &str) -> Result<pnpm_config::ColorMode, &'static str> {
    match value {
        "always" | "true" => Ok(pnpm_config::ColorMode::Always),
        "auto" => Ok(pnpm_config::ColorMode::Auto),
        "never" | "false" => Ok(pnpm_config::ColorMode::Never),
        _ => Err("expected one of: auto, always, never"),
    }
}

impl CliArgs {
    /// The reporter the command should drive: `--loglevel silent` forces
    /// the silent reporter over any `--reporter` choice, mirroring the
    /// reporter selection in pnpm 11's `main.ts`.
    pub(crate) fn effective_reporter(&self) -> ReporterType {
        if self.loglevel == Some(LogLevelSetting::Silent) {
            return ReporterType::Silent;
        }
        self.reporter
    }

    pub fn validate_command_scoped_global_options(&self) -> Result<(), clap::Error> {
        if self.resume_from.is_some() {
            self.validate_run_scoped_global_option("--resume-from")?;
        }
        if self.report_summary {
            self.validate_report_summary_global_option()?;
        }
        if self.no_bail {
            self.validate_no_bail_global_option()?;
        }
        if self.if_present {
            self.validate_if_present_top_level_option()?;
        }
        if self.parallel {
            self.validate_parallel_global_option()?;
        }
        if self.reporter_hide_prefix {
            self.validate_run_scoped_global_option("--reporter-hide-prefix")?;
        }
        if self.no_reporter_hide_prefix {
            self.validate_run_scoped_global_option("--no-reporter-hide-prefix")?;
        }
        Ok(())
    }

    /// Promote the command to recursive mode when a `--filter` /
    /// `--filter-prod` selector is present, even without an explicit
    /// `-r` / `--recursive`.
    ///
    /// Setting `recursive = true` whenever a filter is given applies
    /// CLI-wide rather than being special-cased per command. Call once on
    /// the parsed args before dispatch; both the install fast-path bail
    /// and [`Self::run`] then observe the promoted flag.
    pub fn promote_recursive_for_filter(&mut self) {
        if !self.filter.is_empty() || !self.filter_prod.is_empty() {
            self.recursive = true;
        }
    }

    /// Apply the recursive-run settings represented by pnpm's
    /// `--parallel` shorthand.
    pub fn apply_parallel_run_options(&mut self) {
        if self.parallel {
            self.recursive = true;
            self.no_sort = true;
            self.stream = true;
        }
    }

    /// Apply `--workspace-root` / `-w`: point `--dir` at the workspace
    /// root so the command runs on the root project. Call after
    /// [`Self::promote_recursive_for_filter`] and before anything reads
    /// `--dir`, matching where pnpm's CLI parser applies it.
    ///
    /// The `--dir` resolution mirrors pnpm's `findWorkspaceDir`: real path
    /// first, because a case-insensitive filesystem otherwise finds the
    /// root under one spelling and the members under another; then a
    /// lexical fallback, so an unresolvable `--dir` is not fatal.
    ///
    /// The fallback resolves `..` itself rather than leaving the components
    /// in place. [`pnpm_workspace::find_workspace_dir`] walks ancestors
    /// lexically, so a `--dir` that climbs out of the workspace and lands
    /// on a directory that does not exist — `../../elsewhere` — would
    /// otherwise walk right back up through its own `..` components and
    /// select the workspace the user pointed away from.
    pub fn apply_workspace_root(&mut self) -> Result<(), WorkspaceRootError> {
        if !self.workspace_root {
            return Ok(());
        }
        if self.command.is_global() {
            return Err(WorkspaceRootError::GlobalConflict);
        }
        let dir = dunce::canonicalize(&self.dir)
            .or_else(|_| std::path::absolute(&self.dir))
            .unwrap_or_else(|_| self.dir.clone())
            .pipe_deref(pnpm_fs::lexical_normalize);
        let workspace_dir = pnpm_workspace::find_workspace_dir(&dir)
            .map_err(WorkspaceRootError::FindWorkspaceDir)?
            .ok_or(WorkspaceRootError::NotInWorkspace)?;
        self.dir = workspace_dir;
        Ok(())
    }

    /// Promote commands marked recursive-by-default by pnpm when they run
    /// inside a workspace.
    pub fn promote_recursive_by_default(&mut self) {
        let dir = dunce::canonicalize(&self.dir)
            .or_else(|_| std::path::absolute(&self.dir))
            .unwrap_or_else(|_| self.dir.clone())
            .pipe_deref(pnpm_fs::lexical_normalize);
        if !self.recursive
            && self.command.recursive_by_default()
            && pnpm_workspace::find_workspace_dir(&dir).is_ok_and(|dir| dir.is_some())
        {
            self.recursive = true;
        }
    }

    /// `restart` also runs scripts and accepts its own `--if-present`,
    /// so the top-level spelling is valid for it too — unlike the
    /// recursive-only flags, which `restart` rejects. `exec` is the
    /// reverse: it takes the recursive-only flags but runs arbitrary
    /// commands rather than scripts, so pnpm rejects `--if-present`
    /// for it and pacquet must too.
    fn validate_if_present_top_level_option(&self) -> Result<(), clap::Error> {
        match self.command {
            CliCommand::Restart(_) => Ok(()),
            CliCommand::Exec(_) => Err(Self::unexpected_argument_error("--if-present")),
            _ => self.validate_run_scoped_global_option("--if-present"),
        }
    }

    fn validate_run_scoped_global_option(&self, option: &str) -> Result<(), clap::Error> {
        if matches!(
            self.command,
            CliCommand::Run(_)
                | CliCommand::Exec(_)
                | CliCommand::External(_)
                | CliCommand::Test(_)
                | CliCommand::Start(_)
                | CliCommand::Stop(_),
        ) {
            return Ok(());
        }
        Err(Self::unexpected_argument_error(option))
    }

    fn unexpected_argument_error(option: &str) -> clap::Error {
        Self::command()
            .error(ErrorKind::UnknownArgument, format!("unexpected argument '{option}' found"))
    }

    fn validate_report_summary_global_option(&self) -> Result<(), clap::Error> {
        if matches!(self.command, CliCommand::Publish(_) | CliCommand::Stage(_)) {
            return Ok(());
        }
        self.validate_run_scoped_global_option("--report-summary")
    }

    fn validate_no_bail_global_option(&self) -> Result<(), clap::Error> {
        if matches!(self.command, CliCommand::Rebuild(_)) {
            return Ok(());
        }
        self.validate_run_scoped_global_option("--no-bail")
    }

    fn validate_parallel_global_option(&self) -> Result<(), clap::Error> {
        if matches!(
            self.command,
            CliCommand::Run(_)
                | CliCommand::Exec(_)
                | CliCommand::External(_)
                | CliCommand::Test(_)
                | CliCommand::Start(_)
                | CliCommand::Stop(_),
        ) {
            return Ok(());
        }
        Err(Self::unexpected_argument_error("--parallel"))
    }
}

/// Error type of [`CliArgs::apply_workspace_root`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum WorkspaceRootError {
    #[display("--workspace-root may not be used with --global")]
    #[diagnostic(code(ERR_PNPM_OPTIONS_CONFLICT))]
    GlobalConflict,

    #[display("--workspace-root may only be used inside a workspace")]
    #[diagnostic(code(ERR_PNPM_NOT_IN_WORKSPACE))]
    NotInWorkspace,

    #[diagnostic(transparent)]
    FindWorkspaceDir(#[error(source)] pnpm_workspace::FindWorkspaceDirError),
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Manage package access and visibility on the registry.
    Access(AccessArgs),
    /// Initialize a package.json
    Init(InitArgs),
    /// Concurrently runs a command in all subdirectory projects.
    #[clap(visible_aliases = ["multi", "m"])]
    Recursive,
    /// Add a package
    Add(AddArgs),
    /// Install packages
    #[clap(visible_alias = "i")]
    Install(InstallArgs),
    /// Runs a `pnpm install` followed immediately by a `pnpm test`. It takes exactly the same arguments as `pnpm install`.
    #[clap(name = "install-test", visible_alias = "it")]
    InstallTest(InstallTestArgs),
    /// Update packages to their newest version based on the specified range
    #[clap(visible_aliases = ["up", "upgrade"])]
    Update(UpdateArgs),
    /// Check for outdated package and GitHub Actions dependencies
    Outdated(OutdatedArgs),
    /// Checks for known security issues with the installed packages.
    Audit(AuditArgs),
    /// Record a change intent: which packages a change affects, the bump
    /// type for each, and a summary that becomes the changelog entry.
    Change(ChangeArgs),
    /// Apply the pending change intents (`pnpm version -r`).
    Version(VersionArgs),
    /// Manage per-package release lanes.
    Lane(LaneArgs),
    /// Opens the bug tracker URL of a package in the default browser.
    #[clap(visible_alias = "issues")]
    Bugs(BugsArgs),
    /// List installed packages.
    #[clap(visible_alias = "ls")]
    List(ListArgs),
    /// List installed packages in long format.
    #[clap(visible_alias = "la")]
    Ll(ListArgs),
    /// Check the licenses of the installed packages.
    #[clap(visible_aliases = ["licences"])]
    Licenses(LicensesArgs),
    /// Shows the packages that depend on `pkg`
    Why(WhyArgs),
    /// View registry information about a package.
    #[clap(visible_aliases = ["info", "show", "v"])]
    View(ViewArgs),
    /// Generate a Software Bill of Materials (SBOM).
    Sbom(SbomArgs),
    /// Displays your pnpm username.
    Whoami,
    /// Deprecates a version of a package in the registry.
    Deprecate(DeprecateArgs),
    /// Removes deprecation from a version of a package in the registry. Only works on already deprecated versions.
    Undeprecate(UndeprecateArgs),
    /// Removes a package from the registry.
    Unpublish(UnpublishArgs),
    /// Marks a package as a favorite.
    Star(StarArgs),
    /// Unmarks a package as a favorite.
    Unstar(UnstarArgs),
    /// Lists all packages starred by a specific user.
    Stars(StarsArgs),
    /// Manage a package's distribution tags.
    #[clap(name = "dist-tag", visible_alias = "dist-tags")]
    DistTag(DistTagArgs),
    /// Test connectivity to the configured registry.
    Ping(PingArgs),
    /// Run diagnostics on the pnpm installation and environment.
    Doctor(DoctorArgs),
    /// Search for packages in the registry.
    #[clap(visible_aliases = ["s", "se", "find"])]
    Search(SearchArgs),
    /// Rebuild a package.
    #[clap(visible_alias = "rb")]
    Rebuild(RebuildArgs),
    /// Create a tarball from a package
    Pack(PackArgs),
    /// Publish a package to the registry
    Publish(PublishArgs),
    /// Stage packages for publishing, deferring proof-of-presence (2FA) to a
    /// later point in time.
    Stage(StageArgs),
    /// Removes packages from `node_modules` and from the project's `package.json`.
    // Unlike npm, pnpm does not treat "r" as an alias of "remove" to avoid
    // confusion with "run" and "recursive".
    #[clap(visible_aliases = ["uninstall", "rm", "un", "uni"])]
    Remove(RemoveArgs),
    /// Prepare a package for patching.
    Patch(PatchArgs),
    /// Generate a patch out of a directory.
    #[clap(name = "patch-commit")]
    PatchCommit(PatchCommitArgs),
    /// Remove existing patch files.
    #[clap(name = "patch-remove")]
    PatchRemove(PatchRemoveArgs),
    /// Checks for unmet or missing peer dependency issues.
    #[clap(name = "peers")]
    Peers(PeersArgs),
    /// Set a script in package.json
    #[clap(visible_alias = "ss")]
    SetScript(SetScriptArgs),
    /// Runs a package's "test" script, if one was provided.
    Test(ScriptShortcutArgs),
    /// Runs a defined package script.
    Run(RunArgs),
    /// Run a shell command in the context of a project.
    Exec(ExecArgs),
    /// Run a package in a temporary environment.
    Dlx(DlxArgs),
    /// Creates a project from a `create-*` starter kit.
    Create(CreateArgs),
    /// Print shell completion code to stdout.
    Completion(CompletionArgs),
    /// Dynamic completion endpoint used by generated shell scripts.
    #[clap(name = "completion-server", hide = true)]
    CompletionServer(CompletionServerArgs),
    /// Runs an arbitrary command specified in the package's start property of its scripts object.
    Start(ScriptShortcutArgs),
    /// Runs a package's "stop" script, if one was provided.
    Stop(ScriptShortcutArgs),
    /// Restarts a package. Runs "stop", "restart", and "start" scripts,
    /// and associated pre- and post- scripts.
    Restart(RestartArgs),
    /// Lists the packages that include the file with the specified hash.
    FindHash(FindHashArgs),
    /// Manage runtimes.
    #[clap(visible_alias = "rt")]
    Runtime(RuntimeArgs),
    /// Manage Node.js versions. Deprecated in favour of `pnpm runtime`.
    Env(EnvArgs),
    /// Manage context-aware shims for packages that are not installed
    /// globally, so a project decides which version runs.
    Shim(ShimArgs),
    /// Print the directory where pnpm will install executables.
    Bin(BinArgs),
    /// Safely remove `node_modules` directories from the current project
    /// (or every workspace project) without following NTFS junctions into
    /// their targets. A `clean` script in `package.json` overrides
    /// the built-in command.
    Clean(CleanArgs),
    /// Alias of `clean`: same behavior, except a `purge` script
    /// (not a `clean` script) overrides it when present.
    #[clap(name = "purge")]
    Purge(CleanArgs),
    /// Runs clean then install with a frozen lockfile.
    #[clap(visible_aliases = ["clean-install", "ic", "install-clean"])]
    Ci(CiArgs),
    /// Print the effective `node_modules` directory.
    Root(RootArgs),
    /// Print the current package prefix.
    Prefix(PrefixArgs),
    /// Manage the pnpm configuration files.
    #[clap(visible_alias = "c")]
    Config(ConfigArgs),
    /// Print the config value for the provided key. Shorthand for
    /// `pnpm config get`.
    Get(ConfigGetAliasArgs),
    /// Set the config key to the value provided. Shorthand for
    /// `pnpm config set`.
    Set(ConfigSetAliasArgs),
    /// Manages your package.json.
    Pkg(PkgArgs),
    /// Pack a `CommonJS` entry file into a standalone executable for one or more target platforms.
    #[clap(name = "pack-app")]
    PackApp(PackAppArgs),
    /// Managing the package store.
    #[clap(subcommand)]
    Store(StoreCommand),
    /// Inspect and manage the metadata cache.
    #[clap(subcommand)]
    Cache(CacheCommand),
    /// Prints the contents of a file based on the hash value stored in the index file.
    CatFile(CatFileArgs),
    /// Prints the index file of a specific package from the store.
    CatIndex(CatIndexArgs),
    /// Print the list of packages with blocked build scripts.
    IgnoredBuilds(IgnoredBuildsArgs),
    /// Approve dependencies for running scripts during installation.
    ApproveBuilds(ApproveBuildsArgs),
    /// Links a local package as a dependency
    #[clap(visible_aliases = ["ln"])]
    Link(LinkArgs),
    /// Generates a pnpm-lock.yaml from an external lockfile
    Import(ImportArgs),
    /// Deduplicate packages in the lockfile
    Dedupe(DedupeArgs),
    /// Deploy a package from a workspace
    Deploy(DeployArgs),
    /// Remove extraneous packages
    Prune(PruneArgs),
    /// Fetch packages from the lockfile into the virtual store
    Fetch(FetchArgs),
    /// Removes links to a local package and reinstalls it
    #[clap(visible_aliases = ["dislink"])]
    Unlink(UnlinkArgs),
    /// Opens the documentation of a package in the browser.
    #[clap(visible_alias = "home")]
    Docs(DocsArgs),
    /// Opens the URL of the package's repository in a browser.
    Repo(RepoArgs),
    /// Updates pnpm to the latest version (or the one specified)
    SelfUpdate(SelfUpdateArgs),
    /// Sets up pnpm
    Setup(SetupArgs),
    /// Log in to an npm registry.
    #[clap(visible_alias = "adduser")]
    Login(LoginArgs),
    /// Manage organization teams and team memberships.
    Team(TeamArgs),
    /// Manage package owners on the registry.
    #[clap(visible_alias = "owners")]
    Owner(OwnerArgs),
    /// Log out of an npm registry.
    Logout(LogoutArgs),
    /// Runs pnpm at a specific version (or the currently running one) for a
    /// single invocation, ignoring the "packageManager" and
    /// "devEngines.packageManager" fields of the project's manifest.
    With(WithArgs),
    /// Not implemented in pnpm. Use the npm CLI directly.
    // Registered rather than left to the external-subcommand fallback so
    // it names npm instead of failing as a missing package script.
    Edit(NotImplementedArgs),
    /// Not implemented in pnpm. Use the npm CLI directly.
    Profile(NotImplementedArgs),
    /// Not implemented in pnpm. Use the npm CLI directly.
    Token(NotImplementedArgs),
    /// Not implemented in pnpm. Use the npm CLI directly.
    Xmas(NotImplementedArgs),
    #[clap(external_subcommand)]
    External(Vec<String>),
}

impl CliCommand {
    /// Whether `--global` was passed. pnpm parses it as one CLI-wide
    /// option; pacquet declares it per subcommand.
    fn is_global(&self) -> bool {
        match self {
            CliCommand::Add(args) => args.global,
            CliCommand::ApproveBuilds(args) => args.global,
            CliCommand::Bin(args) => args.global,
            CliCommand::Config(args) => args.is_global(),
            CliCommand::Env(args) => args.global,
            CliCommand::Get(args) => args.flags.global,
            CliCommand::Set(args) => args.flags.global,
            CliCommand::List(args) | CliCommand::Ll(args) => args.global,
            CliCommand::Outdated(args) => args.global,
            CliCommand::Prefix(args) => args.global,
            CliCommand::Remove(args) => args.global,
            CliCommand::Root(args) => args.global,
            CliCommand::Runtime(args) => args.global,
            CliCommand::Update(args) => args.global,
            _ => false,
        }
    }

    pub(super) fn recursive_by_default(&self) -> bool {
        matches!(
            self,
            CliCommand::Install(_)
                | CliCommand::InstallTest(_)
                | CliCommand::Import(_)
                | CliCommand::List(_)
                | CliCommand::Ll(_)
                | CliCommand::Why(_)
                | CliCommand::Peers(_)
                | CliCommand::Ci(_),
        )
    }

    /// Whether this run prints the `Scope:` line naming the workspace
    /// projects it selected. pnpm gates this on two things at once: the
    /// command must be one of the few that report scope, and the run must
    /// be workspace-wide — either asked for with `-r` / `--filter`, or
    /// because the command is workspace-wide by nature (`install`, `prune`).
    pub(crate) fn reports_scope(&self, recursive: bool) -> bool {
        let reports_scope = matches!(
            self,
            CliCommand::Install(_)
                | CliCommand::Link(_)
                | CliCommand::Prune(_)
                | CliCommand::Rebuild(_)
                | CliCommand::Remove(_)
                | CliCommand::Unlink(_)
                | CliCommand::Update(_)
                | CliCommand::Run(_)
                | CliCommand::Test(_),
        );
        reports_scope
            && (recursive || matches!(self, CliCommand::Install(_) | CliCommand::Prune(_)))
    }

    /// Whether reporter output (warnings, progress) goes to stderr so this
    /// command's stdout stays a clean, machine-readable value — pnpm's
    /// `COMMANDS_WITH_STDERR_REPORTER`.
    pub(crate) fn uses_stderr_reporter(&self) -> bool {
        matches!(
            self,
            CliCommand::Dlx(_)
                | CliCommand::Create(_)
                | CliCommand::Config(_)
                | CliCommand::Get(_)
                | CliCommand::Set(_)
                | CliCommand::Sbom(_)
                | CliCommand::Shim(_)
                | CliCommand::With(_)
                | CliCommand::Store(_)
                | CliCommand::Prefix(_)
                | CliCommand::Root(_)
                | CliCommand::Bin(_),
        )
    }

    pub(crate) fn default_reporter_summary_scope(&self) -> SummaryScope {
        match self {
            CliCommand::Access(_) => SummaryScope::CurrentPrefix,
            CliCommand::Star(_) | CliCommand::Stars(_) | CliCommand::Unstar(_) => {
                SummaryScope::CurrentPrefix
            }
            CliCommand::Add(args) if args.global => SummaryScope::AllPrefixes,
            CliCommand::Remove(args) if args.global => SummaryScope::AllPrefixes,
            CliCommand::Runtime(args) if args.global => SummaryScope::AllPrefixes,
            CliCommand::Env(args) if args.global => SummaryScope::AllPrefixes,
            CliCommand::Update(args) if args.global => SummaryScope::AllPrefixes,
            CliCommand::Dlx(_) | CliCommand::Create(_) => SummaryScope::AllPrefixes,
            _ => SummaryScope::CurrentPrefix,
        }
    }
}
