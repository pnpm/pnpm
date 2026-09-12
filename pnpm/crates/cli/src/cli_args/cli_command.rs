pub(super) mod options;

pub use commands::CliCommand;

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
    pipeline::PipelineArgs,
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

/// Package manager.
#[derive(Debug, Parser)]
#[clap(name = "pnpm")]
#[clap(bin_name = "pnpm")]
#[clap(version = pnpm_config::PNPM_VERSION)]
#[clap(disable_version_flag = true)]
#[clap(about = "Package manager")]
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

    /// Whether `--dir` came from the command line rather than from its
    /// default. pnpm keeps that distinction: a `--dir` it was given is
    /// taken as is, while the default resolves to the local prefix and
    /// `init` scaffolds in the process cwd. `clap` cannot report it
    /// through a derived field, so the entry point fills it in from the
    /// parsed matches.
    #[clap(skip)]
    pub dir_from_command_line: bool,

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

impl CliArgs {}

mod commands;
