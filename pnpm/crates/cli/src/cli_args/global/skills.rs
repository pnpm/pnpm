use miette::{
    Context,
    IntoDiagnostic,
};
use pnpm_modules_yaml::{
    Host,
    read_modules_layout,
    read_modules_manifest,
    write_modules_manifest,
};
use std::{
    collections::BTreeSet,
    path::Path,
};

pub(crate) fn links_of_other_groups(
    global_pkg_dir: &Path,
    excluded: &BTreeSet<&Path>,
) -> miette::Result<BTreeSet<String>> {
    let groups = super::scan_global_packages(global_pkg_dir)
        .into_diagnostic()
        .wrap_err("scan global skill link ownership")?;
    let mut links = BTreeSet::new();
    for group in groups {
        if !excluded.contains(group.install_dir.as_path()) {
            links.extend(recorded_links(&group.install_dir)?);
        }
    }
    Ok(links)
}

pub(super) fn recorded_group_links<'a>(
    dirs: impl IntoIterator<Item = &'a Path>,
) -> miette::Result<Vec<String>> {
    let mut links = BTreeSet::new();
    for dir in dirs {
        links.extend(recorded_links(dir)?);
    }
    Ok(links.into_iter().collect())
}

fn recorded_links(install_dir: &Path) -> miette::Result<Vec<String>> {
    read_modules_layout::<Host>(&install_dir.join("node_modules"))
        .into_diagnostic()
        .wrap_err_with(|| format!("read skill links for {}", install_dir.display()))
        .map(|modules| modules.and_then(|modules| modules.linked_skills).unwrap_or_default())
}

pub(super) fn prepare_skill_link_replacement(
    global_pkg_dir: &Path,
    candidate_dir: &Path,
    replaced: &[super::remove::GlobalPackageBinSnapshot],
) -> miette::Result<BTreeSet<String>> {
    let replaced_dirs: BTreeSet<_> = replaced
        .iter()
        .map(|group| group.info.install_dir.as_path())
        .collect();
    let surviving_links = links_of_other_groups(global_pkg_dir, &replaced_dirs)?;
    inherit_links(candidate_dir, recorded_group_links(replaced_dirs)?)?;
    Ok(surviving_links)
}

/// Keep ownership of old links recoverable immediately after replacing a group.
fn inherit_links(install_dir: &Path, links: Vec<String>) -> miette::Result<()> {
    if links.is_empty() {
        return Ok(());
    }
    let modules_dir = install_dir.join("node_modules");
    let mut modules = read_modules_manifest::<Host>(&modules_dir)
        .into_diagnostic()?
        .ok_or_else(|| miette::miette!("missing installed state at {}", modules_dir.display()))?;
    modules.linked_skills = Some(links);
    write_modules_manifest::<Host>(&modules_dir, modules).into_diagnostic()
}
