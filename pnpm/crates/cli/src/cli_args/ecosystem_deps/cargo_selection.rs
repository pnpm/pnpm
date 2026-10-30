use super::{
    cargo::CargoUpdateOptions,
    cargo_manifest_ops::{DeclaredCargoDep, same_declared_dep},
    cargo_workspace_scope::CargoWorkspaceScope,
};
use miette::{Result, bail};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use semver::Version;

/// A dependency `pnpm update` is to update.
pub(super) struct SelectedCargoDep<'a> {
    pub(super) dep: &'a DeclaredCargoDep,
    pub(super) explicit_spec: Option<&'a str>,
    /// Whether the command line names the crate rather than reaching it
    /// through a pattern or an update of everything.
    pub(super) named: bool,
}

pub(super) fn select_cargo_deps_to_update<'a>(
    declared: &'a [DeclaredCargoDep],
    options: &'a CargoUpdateOptions<'a>,
) -> Vec<SelectedCargoDep<'a>> {
    let mut selected: Vec<SelectedCargoDep<'a>> = Vec::new();
    for dep in declared {
        if let Some(selection) = match_cargo_dep(dep, options)
            && !selected.iter().any(|sel| same_declared_dep(sel.dep, dep))
        {
            selected.push(selection);
        }
    }
    selected
}

fn match_cargo_dep<'a>(
    dep: &'a DeclaredCargoDep,
    options: &'a CargoUpdateOptions<'a>,
) -> Option<SelectedCargoDep<'a>> {
    if !options.step.include_groups.contains(&dep.group)
        || options.step.ignore_matcher.is_some_and(|matcher| matcher.matches(&dep.alias))
    {
        return None;
    }
    let unnamed = SelectedCargoDep { dep, explicit_spec: None, named: false };
    if !options.restrict_to_selectors {
        return Some(unnamed);
    }
    if let Some(req) = options.requests
        .iter()
        .find(|req| req.name == dep.alias || req.name == dep.package_name)
    {
        return Some(SelectedCargoDep {
            dep,
            explicit_spec: req.version_spec.as_deref(),
            named: true,
        });
    }
    options.bare_matcher
        .is_some_and(|matcher| matcher.matches(&dep.alias))
        .then_some(unnamed)
}

pub(super) struct SelectedCargoTarget<'a> {
    pub(super) selection: &'a SelectedCargoDep<'a>,
    pub(super) scope: &'a CargoWorkspaceScope,
}

/// Whether `version` defines every feature the workspace enables on the
/// selected dependency. Cargo cannot resolve a declaration to a version
/// missing one, so a crate the command line names fails here, and any
/// other crate keeps its version with a warning.
pub(super) fn admits_enabled_features<Reporter: self::Reporter>(
    target: &SelectedCargoTarget<'_>,
    index_file: &str,
    version: &Version,
) -> Result<bool> {
    let dep = target.selection.dep;
    let enabled = target.scope.enabled_features(dep);
    if enabled.is_empty() {
        return Ok(true);
    }
    let missing =
        pnpm_cargo_resolver::missing_features(&dep.package_name, index_file, version, &enabled)?;
    if missing.is_empty() {
        return Ok(true);
    }
    let crate_version = if dep.alias == dep.package_name {
        format!("{} {version}", dep.alias)
    } else {
        format!("{} ({} {version})", dep.alias, dep.package_name)
    };
    let features = missing
        .iter()
        .map(|feature| format!("`{feature}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let reason = format!("it does not have the features this workspace enables on it: {features}");
    if target.selection.named {
        bail!("cannot update crate {crate_version}: {reason}");
    }
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: format!("Not updating crate {crate_version}: {reason}"),
        prefix: target.scope.root.to_string_lossy().into_owned(),
    }));
    Ok(false)
}
