use super::{
    add::AddArgs,
    approve_builds::ApproveBuildsArgs,
    audit::AuditArgs,
    bin::BinArgs,
    bugs::BugsArgs,
    cache::CacheCommand,
    cat_file::CatFileArgs,
    cat_index::CatIndexArgs,
    completion::{CompletionArgs, CompletionServerArgs},
    config::ConfigArgs,
    create::CreateArgs,
    dedupe::DedupeArgs,
    deploy::DeployArgs,
    dist_tag::DistTagArgs,
    dlx::DlxArgs,
    docs::DocsArgs,
    exec::ExecArgs,
    fetch::FetchArgs,
    find_hash::FindHashArgs,
    ignored_builds::IgnoredBuildsArgs,
    import::ImportArgs,
    install::InstallArgs,
    link::LinkArgs,
    list::ListArgs,
    logout::LogoutArgs,
    outdated::OutdatedArgs,
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
    reporter::ReporterType,
    restart::RestartArgs,
    root::RootArgs,
    run::RunArgs,
    runtime::RuntimeArgs,
    sbom::SbomArgs,
    self_update::SelfUpdateArgs,
    set_script::SetScriptArgs,
    setup::SetupArgs,
    stop::StopArgs,
    store::StoreCommand,
    unlink::UnlinkArgs,
    update::UpdateArgs,
    why::WhyArgs,
    with::WithArgs,
};
use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};
use pacquet_default_reporter::SummaryScope;
use std::path::PathBuf;

/// Experimental package manager for node.js written in rust.
#[derive(Debug, Parser)]
#[clap(name = "pnpm")]
#[clap(bin_name = "pnpm")]
#[clap(version = pacquet_config::PACQUET_VERSION)]
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
    #[clap(long, global = true)]
    pub color: bool,

    /// Automatically answer yes to prompts.
    #[clap(short = 'y', long, global = true)]
    pub yes: bool,

    /// Set working directory.
    #[clap(short = 'C', long, default_value = ".")]
    pub dir: PathBuf,

    /// Path to a `.npmrc` to read auth settings from, overriding the
    /// default `~/.npmrc`. Mirrors pnpm's `--npmrc-auth-file` (and its
    /// `--userconfig` alias) and sets
    /// [`pacquet_config::Config::npmrc_auth_file`], consumed when
    /// `Config` resolves the user-level `.npmrc`.
    #[clap(long = "npmrc-auth-file", visible_alias = "userconfig", global = true)]
    pub npmrc_auth_file: Option<PathBuf>,

    /// Run the command for every project in the workspace instead of
    /// only the project in `--dir`. Mirrors pnpm's global `-r` /
    /// `--recursive` flag and sets
    /// [`pacquet_config::Config::recursive`]. pacquet's `install`
    /// already spans the whole workspace, so the flag is a surface
    /// no-op there today; see the field docs.
    #[clap(short = 'r', long, global = true)]
    pub recursive: bool,

    /// Reporter output format.
    #[clap(long, value_enum, default_value_t = ReporterType::Default, global = true)]
    pub reporter: ReporterType,

    /// `--filter` / `-F` workspace selectors. Each occurrence adds one
    /// raw selector (`@scope/*`, `./pkg`, `foo...`, `!bar`, `{dir}`,
    /// `[since]`, ...). Stored into [`pacquet_config::Config::filter`];
    /// see that field for why the resolved selection is not yet
    /// consumed by `install`.
    ///
    /// As a global multi-value flag, occurrences collect only within one
    /// side of the subcommand boundary; mixing sides is a clap limitation,
    /// so pass all selectors on the same side.
    #[clap(short = 'F', long, global = true)]
    pub filter: Vec<String>,

    /// `--filter-prod` workspace selectors. Same syntax as
    /// [`Self::filter`], but the dependency walk follows production
    /// dependencies only. Stored into
    /// [`pacquet_config::Config::filter_prod`].
    #[clap(long = "filter-prod", global = true)]
    pub filter_prod: Vec<String>,

    /// Keep recursive workspace projects sorted topologically.
    #[clap(long = "sort", global = true, overrides_with = "no_sort")]
    pub sort: bool,

    /// Run recursive workspace projects in workspace order.
    #[clap(long = "no-sort", global = true, overrides_with = "sort")]
    pub no_sort: bool,

    /// Maximum number of workspace projects to process in parallel.
    #[clap(long = "workspace-concurrency", global = true)]
    pub workspace_concurrency: Option<i32>,

    /// Recursive only: resume execution from the given package.
    #[clap(long = "resume-from", global = true, hide = true)]
    pub resume_from: Option<String>,

    /// Recursive only: write a `pnpm-exec-summary.json` execution report.
    #[clap(long = "report-summary", global = true, hide = true)]
    pub report_summary: bool,

    /// Recursive only: keep going after a project fails.
    #[clap(long = "no-bail", global = true, hide = true)]
    pub no_bail: bool,
}

impl CliArgs {
    pub fn validate_command_scoped_global_options(&self) -> Result<(), clap::Error> {
        if self.resume_from.is_some() {
            self.validate_run_scoped_global_option("--resume-from")?;
        }
        if self.report_summary {
            self.validate_report_summary_global_option()?;
        }
        if self.no_bail {
            self.validate_run_scoped_global_option("--no-bail")?;
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

    fn validate_run_scoped_global_option(&self, option: &str) -> Result<(), clap::Error> {
        if matches!(
            self.command,
            CliCommand::Run(_)
                | CliCommand::Exec(_)
                | CliCommand::External(_)
                | CliCommand::Test
                | CliCommand::Start
                | CliCommand::Stop(_),
        ) {
            return Ok(());
        }
        Err(Self::command()
            .error(ErrorKind::UnknownArgument, format!("unexpected argument '{option}' found")))
    }

    fn validate_report_summary_global_option(&self) -> Result<(), clap::Error> {
        if matches!(self.command, CliCommand::Publish(_)) {
            return Ok(());
        }
        self.validate_run_scoped_global_option("--report-summary")
    }
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Initialize a package.json
    Init,
    /// Add a package
    Add(AddArgs),
    /// Install packages
    #[clap(visible_alias = "i")]
    Install(InstallArgs),
    /// Update packages to their newest version based on the specified range
    #[clap(visible_aliases = ["up", "upgrade"])]
    Update(UpdateArgs),
    /// Check for outdated packages
    Outdated(OutdatedArgs),
    /// Checks for known security issues with the installed packages.
    Audit(AuditArgs),
    /// Opens the bug tracker URL of a package in the default browser.
    #[clap(visible_alias = "issues")]
    Bugs(BugsArgs),
    /// List installed packages.
    #[clap(visible_alias = "ls")]
    List(ListArgs),
    /// List installed packages in long format.
    #[clap(visible_alias = "la")]
    Ll(ListArgs),
    /// Shows the packages that depend on `pkg`
    Why(WhyArgs),
    /// Generate a Software Bill of Materials (SBOM).
    Sbom(SbomArgs),
    /// Displays your pnpm username.
    Whoami,
    /// Manage a package's distribution tags.
    #[clap(name = "dist-tag", visible_alias = "dist-tags")]
    DistTag(DistTagArgs),
    /// Test connectivity to the configured registry.
    Ping(PingArgs),
    /// Rebuild a package.
    #[clap(visible_alias = "rb")]
    Rebuild(RebuildArgs),
    /// Create a tarball from a package
    Pack(PackArgs),
    /// Publish a package to the registry
    Publish(PublishArgs),
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
    Test,
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
    Start,
    /// Runs a package's "stop" script, if one was provided.
    Stop(StopArgs),
    /// Restarts a package. Runs "stop", "restart", and "start" scripts,
    /// and associated pre- and post- scripts.
    Restart(RestartArgs),
    /// Lists the packages that include the file with the specified hash.
    FindHash(FindHashArgs),
    /// Manage runtimes.
    #[clap(visible_alias = "rt")]
    Runtime(RuntimeArgs),
    /// Print the directory where pnpm will install executables.
    Bin(BinArgs),
    /// Print the effective `node_modules` directory.
    Root(RootArgs),
    /// Print the current package prefix.
    Prefix(PrefixArgs),
    /// Manage the pnpm configuration files.
    #[clap(visible_alias = "c")]
    Config(ConfigArgs),
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
    /// Log out of an npm registry.
    Logout(LogoutArgs),
    /// Runs pnpm at a specific version (or the currently running one) for a
    /// single invocation, ignoring the "packageManager" and
    /// "devEngines.packageManager" fields of the project's manifest.
    With(WithArgs),
    #[clap(external_subcommand)]
    External(Vec<String>),
}

impl CliCommand {
    pub(crate) fn default_reporter_summary_scope(&self) -> SummaryScope {
        match self {
            CliCommand::Add(args) if args.global => SummaryScope::AllPrefixes,
            CliCommand::Remove(args) if args.global => SummaryScope::AllPrefixes,
            CliCommand::Runtime(args) if args.global => SummaryScope::AllPrefixes,
            CliCommand::Update(args) if args.global => SummaryScope::AllPrefixes,
            CliCommand::Dlx(_) | CliCommand::Create(_) => SummaryScope::AllPrefixes,
            _ => SummaryScope::CurrentPrefix,
        }
    }
}
