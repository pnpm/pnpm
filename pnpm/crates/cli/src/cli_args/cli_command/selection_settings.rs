use crate::{
    cli_args::grammar,
    flag_relocation::{ArgTable, attached_short_option_prefix, inline_long_option_name},
    parse_boundary::{option_width, passthrough_from},
};
use std::ffi::{OsStr, OsString};

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

/// Extract the command-line interleaving of `--filter` and `--filter-prod`
/// arguments from `argv` across both sides of a subcommand.
pub fn extract_filter_order(
    matches: &clap::ArgMatches,
    argv: &[OsString],
    expected_filter_count: usize,
    expected_prod_count: usize,
) -> Vec<pnpm_config::FilterSource> {
    if expected_filter_count == 0 && expected_prod_count == 0 {
        return Vec::new();
    }
    let arity = matched_arity_table(grammar(), matches);
    let limit = passthrough_from(argv).unwrap_or(argv.len()).min(argv.len());
    let mut order = Vec::with_capacity(expected_filter_count + expected_prod_count);
    let mut index = 1;
    while index < limit {
        let token = &argv[index];
        if token == "--" {
            break;
        }
        if let Some(source) = classify_filter_token(token, &arity) {
            order.push(source);
        }
        index += step_width(
            token,
            argv.get(index + 1)
                .map(OsString::as_os_str),
            &arity,
            limit - index,
        );
    }
    let filter_matches = order
        .iter()
        .filter(|&&source| source == pnpm_config::FilterSource::Filter)
        .count()
        == expected_filter_count;
    let prod_matches = order
        .iter()
        .filter(|&&source| source == pnpm_config::FilterSource::FilterProd)
        .count()
        == expected_prod_count;
    if filter_matches && prod_matches { order } else { Vec::new() }
}

fn matched_arity_table(command: &clap::Command, matches: &clap::ArgMatches) -> ArgTable {
    matches
        .subcommand_name()
        .and_then(|name| command.find_subcommand(name))
        .map_or_else(|| ArgTable::top_level(command), |sub| ArgTable::for_subcommand(command, sub))
}

fn classify_filter_token(token: &OsStr, arity: &ArgTable) -> Option<pnpm_config::FilterSource> {
    if let Some(name) = inline_long_option_name(token) {
        return match name {
            "filter" => Some(pnpm_config::FilterSource::Filter),
            "filter-prod" => Some(pnpm_config::FilterSource::FilterProd),
            _ => None,
        };
    }
    if let Some(long) = token
        .to_str()
        .and_then(|text| text.strip_prefix("--"))
    {
        return match long {
            "filter" => Some(pnpm_config::FilterSource::Filter),
            "filter-prod" => Some(pnpm_config::FilterSource::FilterProd),
            _ => None,
        };
    }
    attached_short_option_prefix(token, |short| arity.short_consumes_value(short))
        .filter(|prefix| prefix.ends_with('F'))
        .map(|_| pnpm_config::FilterSource::Filter)
}

fn step_width(token: &OsStr, next: Option<&OsStr>, arity: &ArgTable, rest: usize) -> usize {
    token
        .to_str()
        .and_then(|text| option_width(text, next.and_then(OsStr::to_str), arity))
        .unwrap_or(1)
        .min(rest)
}
