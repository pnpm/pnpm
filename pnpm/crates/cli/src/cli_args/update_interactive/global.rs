use super::{
    Config, DependencyGroup, HashSet, OutdatedQuery, Path, PromptRow, PromptStyle, Reporter,
    TargetVersion, UpdatePrompt, collect_outdated_for_importer, has_pnpm_cli_dependency,
    ignored_dependencies_matcher, miette, report_cancelled, sanitize_inline, selected_packages,
};

pub(crate) async fn select_global_package_groups<Reporter: self::Reporter>(
    base_config: &'static Config,
    packages: &[String],
    latest: bool,
    prompt: UpdatePrompt,
) -> miette::Result<Option<HashSet<String>>> {
    let global_pkg_dir = base_config.global_pkg_dir.clone().ok_or_else(|| {
        miette!(code = "ERR_PNPM_NO_GLOBAL_BIN_DIR", "Unable to find the global packages directory")
    })?;
    let config = global_update_config(base_config);
    let ignored = ignored_dependencies_matcher(config);
    let query = OutdatedQuery {
        target_version: if latest { TargetVersion::Latest } else { TargetVersion::WithinRange },
        include_direct: &[DependencyGroup::Prod],
        match_names: None,
        ignore_names: ignored.as_ref(),
        include_deprecated: false,
    };
    let Some(matched_packages) = matching_global_packages(&global_pkg_dir, packages)? else {
        return Ok(None);
    };
    let rows = outdated_group_rows(matched_packages, config, &query).await?;
    if rows.is_empty() {
        let message = if latest {
            "All of your dependencies are already up to date"
        } else {
            "All of your dependencies are already up to date inside the specified ranges. Use the --latest option to update the ranges in package.json"
        };
        println!("{message}");
        return Ok(None);
    }
    let Some(selected_indices) = prompt.select(
        "Choose which global package groups to update (space to select, enter to confirm)",
        &rows,
        PromptStyle::GlobalGroups,
    )?
    else {
        report_cancelled::<Reporter>();
        return Ok(None);
    };
    let selected = selected_packages(&rows, &selected_indices).into_iter().collect::<HashSet<_>>();
    if selected.is_empty() {
        return Ok(None);
    }
    Ok(Some(selected))
}

/// One prompt row per global group that has an update available, labelled
/// with the packages the group would move.
async fn outdated_group_rows(
    matched_packages: Vec<pnpm_global::GlobalPackageInfo>,
    config: &'static Config,
    query: &OutdatedQuery<'_>,
) -> miette::Result<Vec<PromptRow>> {
    let mut rows = Vec::new();
    for pkg in matched_packages {
        let state = crate::State::init(pkg.install_dir.join("package.json"), config, false)
            .map_err(|err| miette::Report::new(err).wrap_err("initialize global state"))?;
        let lockfile = state
            .lockfile
            .get()
            .map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))?;
        let outdated = collect_outdated_for_importer(
            &state.manifest,
            lockfile,
            pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY,
            config,
            &state.http_client,
            query,
        )
        .await?;
        if outdated.is_empty() {
            continue;
        }
        let label = outdated
            .iter()
            .map(|package| {
                format!(
                    "{} {} → {}",
                    sanitize_inline(&package.alias),
                    package.current,
                    package.target,
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        rows.push(PromptRow::Choice { short: label.clone(), label, value: pkg.hash });
    }
    Ok(rows)
}

/// The global package groups the params select, or `None` after printing
/// why there is nothing to update. A global group is always updated as a
/// whole, so the params select groups rather than dependencies, the same
/// way `handle_global_update` reads them.
fn matching_global_packages(
    global_pkg_dir: &Path,
    packages: &[String],
) -> miette::Result<Option<Vec<pnpm_global::GlobalPackageInfo>>> {
    let global_packages = pnpm_global::scan_global_packages(global_pkg_dir)
        .map_err(|err| miette!("failed to scan global packages: {err}"))?;
    if global_packages.is_empty() {
        println!("No global packages found");
        return Ok(None);
    }
    let global_packages: Vec<_> =
        global_packages.into_iter().filter(|pkg| !has_pnpm_cli_dependency(pkg)).collect();
    if global_packages.is_empty() {
        println!(r#"No global packages to update. Run "pnpm self-update" to update pnpm itself."#);
        return Ok(None);
    }
    if packages.is_empty() {
        return Ok(Some(global_packages));
    }
    let matched = global_packages
        .into_iter()
        .filter(|pkg| packages.iter().any(|param| pkg.has_alias(param)))
        .collect::<Vec<_>>();
    if matched.is_empty() {
        println!("No matching global packages found");
        return Ok(None);
    }
    Ok(Some(matched))
}

/// Global groups always write a lockfile; read it independently of the caller's
/// project lockfile setting when deciding which installed versions can update.
fn global_update_config(base_config: &Config) -> &'static Config {
    let mut config = base_config.clone();
    config.workspace_dir = None;
    config.shared_workspace_lockfile = false;
    config.lockfile_dir = None;
    // A group's lockfile is written unconditionally (`run_group_install`
    // forces it) because it is where the installed versions are recorded, so
    // reading it back must not depend on the caller's `lockfile` setting.
    config.lockfile = true;
    Config::leak(config)
}
