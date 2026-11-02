use super::{
    cargo::CargoUpdateOptions,
    cargo_manifest_ops::{DeclaredCargoDep, same_declared_dep},
    cargo_workspace_scope::CargoWorkspaceScope,
};
use miette::{IntoDiagnostic, Result, bail};
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use semver::{Op, Version, VersionReq};

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
    pub(super) locked: Option<&'a Version>,
}

/// The requirement the selected dependency's new version must meet, or
/// `None` for the newest release. Without `--latest` it is the current
/// requirement. With it, a renamed alias of a crate the workspace also
/// declares under another name stays on its line unless the command line
/// names it: Cargo rejects a crate that depends on one version under two
/// names.
pub(super) fn target_requirement(
    target: &SelectedCargoTarget<'_>,
    latest: bool,
) -> Result<Option<VersionReq>> {
    let dep = target.selection.dep;
    if let Some(spec) = target.selection.explicit_spec {
        return VersionReq::parse(spec).into_diagnostic().map(Some);
    }
    let keeps_its_line = !target.selection.named && target.scope.has_sibling_alias(dep);
    Ok((!latest || keeps_its_line).then(|| dep.version_req.clone()))
}

/// Whether the update leaves a crate `[patch.crates-io]` replaces alone,
/// with a warning. A new requirement, even one within range, can stop the
/// patch from applying, so only a crate the command line names is updated.
pub(super) fn is_held_by_patch<Reporter: self::Reporter>(target: &SelectedCargoTarget<'_>) -> bool {
    let dep = target.selection.dep;
    if target.selection.named || !target.scope.is_patched(&dep.package_name) {
        return false;
    }
    warn::<Reporter>(
        target.scope,
        format!("Not updating crate {}: `[patch.crates-io]` in Cargo.toml replaces it", dep.alias),
    );
    true
}

/// Whether the update leaves a crate alone, with a warning, because
/// `version` is older than its current version: the locked one, or without
/// a lockfile the lowest version its requirement admits. Only an explicit
/// version moves a crate backwards, as `--latest` would for a prerelease
/// newer than the newest stable release.
pub(super) fn is_held_as_downgrade<Reporter: self::Reporter>(
    target: &SelectedCargoTarget<'_>,
    version: &Version,
) -> bool {
    let dep = target.selection.dep;
    if target.selection.explicit_spec.is_some() {
        return false;
    }
    let current = target.locked.cloned().or_else(|| requirement_floor(&dep.version_req));
    let Some(current) = current.filter(|current| version < current) else {
        return false;
    };
    warn::<Reporter>(
        target.scope,
        format!("Not updating crate {}: {version} is older than {current}", dep.alias),
    );
    true
}

fn requirement_floor(req: &VersionReq) -> Option<Version> {
    req.comparators
        .iter()
        .filter(|comparator| {
            matches!(comparator.op, Op::Exact | Op::GreaterEq | Op::Tilde | Op::Caret)
        })
        .map(|comparator| Version {
            pre: comparator.pre.clone(),
            ..Version::new(
                comparator.major,
                comparator.minor.unwrap_or(0),
                comparator.patch.unwrap_or(0),
            )
        })
        .max()
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
    warn::<Reporter>(target.scope, format!("Not updating crate {crate_version}: {reason}"));
    Ok(false)
}

fn warn<Reporter: self::Reporter>(scope: &CargoWorkspaceScope, message: String) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message,
        prefix: scope.root.to_string_lossy().into_owned(),
    }));
}

#[cfg(test)]
mod tests;
