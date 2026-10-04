/// The selection flags that override a `pnpm-workspace.yaml` setting,
/// each with a `--no-` form that turns the setting back off.
#[derive(Debug, Clone, clap::Args)]
pub struct SelectionSettingArgs {
    /// Exit with code 1 when the `--filter` / `--filter-prod` selectors
    /// match no workspace project.
    #[clap(long = "fail-if-no-match", global = true, overrides_with = "no_fail_if_no_match")]
    pub fail_if_no_match: bool,
    /// Let a command run over an empty selection, overriding a
    /// `failIfNoMatch: true` setting.
    #[clap(long = "no-fail-if-no-match", global = true, overrides_with = "fail_if_no_match")]
    pub no_fail_if_no_match: bool,
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
}
