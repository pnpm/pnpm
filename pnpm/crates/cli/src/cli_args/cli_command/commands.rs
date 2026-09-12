use super::{
    AccessArgs, AddArgs, ApproveBuildsArgs, AuditArgs, BinArgs, BugsArgs, CacheCommand,
    CatFileArgs, CatIndexArgs, ChangeArgs, CiArgs, CleanArgs, CompletionArgs, CompletionServerArgs,
    ConfigArgs, ConfigGetAliasArgs, ConfigSetAliasArgs, CreateArgs, DedupeArgs, DeployArgs,
    DeprecateArgs, DistTagArgs, DlxArgs, DocsArgs, DoctorArgs, EnvArgs, ExecArgs, FetchArgs,
    FindHashArgs, IgnoredBuildsArgs, ImportArgs, InitArgs, InstallArgs, InstallTestArgs, LaneArgs,
    LicensesArgs, LinkArgs, ListArgs, LoginArgs, LogoutArgs, NotImplementedArgs, OutdatedArgs,
    OwnerArgs, PackAppArgs, PackArgs, PatchArgs, PatchCommitArgs, PatchRemoveArgs, PeersArgs,
    PingArgs, PipelineArgs, PkgArgs, PrefixArgs, PruneArgs, PublishArgs, RebuildArgs, RemoveArgs,
    RepoArgs, RestartArgs, RootArgs, RunArgs, RuntimeArgs, SbomArgs, ScriptShortcutArgs,
    SearchArgs, SelfUpdateArgs, SetScriptArgs, SetupArgs, ShimArgs, StageArgs, StarArgs, StarsArgs,
    StoreCommand, Subcommand, SummaryScope, TeamArgs, UndeprecateArgs, UnlinkArgs, UnpublishArgs,
    UnstarArgs, UpdateArgs, VersionArgs, ViewArgs, WhyArgs, WithArgs,
};

#[derive(Debug, strum::IntoStaticStr, Subcommand)]
#[strum(serialize_all = "kebab-case")]
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
    /// Runs a named pipeline of workspace tasks the way a CI run would:
    /// a frozen install, affected-since-base selection, the task graph in
    /// dependency order without bailing, and cached task results restored
    /// instead of re-run.
    Pipeline(PipelineArgs),
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
    pub(super) fn is_global(&self) -> bool {
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

    /// Whether the command's only subject is `package.json` or
    /// `node_modules`, so a directory that holds only a `Cargo.toml` or a
    /// `pyproject.toml` is not a project it can act on.
    ///
    /// Such a command resolves `--dir` through
    /// [`super::super::prefix::find_npm_local_prefix`], which walks past a Cargo
    /// or Python package to the npm project around it. The commands that
    /// install or record dependencies keep the wider walk, so
    /// `pnpm add crate:…` still edits the nearest `Cargo.toml`, and so do
    /// the ones that report across ecosystems, `licenses` and `outdated`
    /// among them.
    pub(super) fn acts_on_the_npm_project(&self) -> bool {
        matches!(
            self,
            CliCommand::Bin(_)
                | CliCommand::Clean(_)
                | CliCommand::Exec(_)
                | CliCommand::External(_)
                | CliCommand::Pkg(_)
                | CliCommand::Purge(_)
                | CliCommand::Restart(_)
                | CliCommand::Root(_)
                | CliCommand::Run(_)
                | CliCommand::SetScript(_)
                | CliCommand::Start(_)
                | CliCommand::Stop(_)
                | CliCommand::Test(_),
        )
    }

    pub(in super::super) fn recursive_by_default(&self) -> bool {
        matches!(
            self,
            CliCommand::Install(_)
                | CliCommand::Dedupe(_)
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
