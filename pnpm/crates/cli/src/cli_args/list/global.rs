use super::{
    ListArgs,
    RecursionLimit,
    ReportAs,
};
use miette::IntoDiagnostic;
use pnpm_config::Config;
use pnpm_global::{
    ListReportAs,
    find_global_install_dirs,
    list_global_packages,
};
use std::path::{
    Path,
    PathBuf,
};

impl ListArgs {
    pub(super) async fn run_global(&self, config: &Config) -> miette::Result<String> {
        let global_pkg_dir = config.global_pkg_dir
            .clone()
            .ok_or_else(|| {
                miette::miette!(
                    code = "ERR_PNPM_NO_GLOBAL_BIN_DIR",
                    "Unable to find the global packages directory"
                )
            })?;

        let tools = crate::cli_args::global::rust::global_tools(config);
        let selects_tool =
            !crate::cli_args::global::rust::selected_tools(&tools, &self.packages).is_empty();
        let report_as = self.report_as();
        if (matches!(self.graph.depth, RecursionLimit::Levels(n) if n > 0)
            || self.graph.depth == RecursionLimit::Unlimited)
            && let Some(output) =
                self.render_global_tree(config, &global_pkg_dir, selects_tool).await?
        {
            return Ok(crate::cli_args::global::rust::with_tools(
                output,
                &tools,
                &self.packages,
                global_report_as(report_as),
            ));
        }

        list_global_packages(
            &global_pkg_dir,
            &self.packages,
            tools,
            global_report_as(report_as),
            self.output.long,
        )
        .into_diagnostic()
    }

    async fn render_global_tree(
        &self,
        config: &Config,
        global_pkg_dir: &Path,
        selects_tool: bool,
    ) -> miette::Result<Option<String>> {
        let Some((install_dir, params)) =
            pick_global_install_dir(global_pkg_dir, &self.packages, selects_tool)?
        else {
            return Ok(None);
        };
        let group_config = global_install_dir_config(config, &install_dir);
        self.render_projects(
            &group_config,
            std::slice::from_ref(&install_dir),
            params,
            &install_dir,
            true,
        )
        .await
        .map(Some)
    }
}

fn pick_global_install_dir<'a>(
    global_pkg_dir: &Path,
    params: &'a [String],
    selects_tool: bool,
) -> miette::Result<Option<(PathBuf, &'a [String])>> {
    let all_install_dirs = find_global_install_dirs(global_pkg_dir, &[]).into_diagnostic()?;
    if let [install_dir] = all_install_dirs.as_slice() {
        // Single global install: keep params so the search can cover the whole
        // tree, matching regular `pnpm ls`.
        return Ok(Some((install_dir.clone(), params)));
    }
    let matching_install_dirs = find_global_install_dirs(global_pkg_dir, params).into_diagnostic()?;
    // Params that select only a tool, such as `rust`, are served by the flat listing.
    if matching_install_dirs.len() > 1
        || (matching_install_dirs.is_empty() && !all_install_dirs.is_empty() && !selects_tool)
    {
        return Err(miette::miette!(
            code = "ERR_PNPM_GLOBAL_LS_DEPTH_NOT_SUPPORTED",
            "Cannot list a merged dependency tree across multiple global packages. \
                 Each global package is installed in an isolated directory with its own lockfile, \
                 so transitive dependencies cannot be coherently merged. \
                 Filter to a single global package by its top-level name, or omit --depth."
        ));
    }
    // Params served their purpose of narrowing to a single install group;
    // passing them on would activate search semantics, which prune the matched
    // package's children.
    Ok(matching_install_dirs
        .into_iter()
        .next()
        .map(|dir| (dir, &[][..])))
}

fn global_report_as(report_as: ReportAs) -> ListReportAs {
    match report_as {
        ReportAs::Tree => ListReportAs::Tree,
        ReportAs::Json => ListReportAs::Json,
        ReportAs::Parseable => ListReportAs::Parseable,
    }
}

/// `config` re-anchored on one isolated global package install directory so
/// tree and dependents inspection read `<install_dir>/node_modules` rather
/// than the pnpm home's or caller project's `modules_dir`.
pub(in crate::cli_args) fn global_install_dir_config(
    config: &Config,
    install_dir: &Path,
) -> Config {
    let mut group_config = config.clone();
    group_config.modules_dir = install_dir.join("node_modules");
    group_config.install_state_dir = group_config.modules_dir.join(".pnpm");
    group_config.workspace_dir = None;
    group_config
}
