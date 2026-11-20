use super::{
    GlobalPackageInfo,
    GlobalUpdateMode,
    HashMap,
    RangeSpecStyle,
    Version,
    local_file_path,
    parse_npm_spec_body,
};
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_resolving_npm_resolver::calc_version_range;
use pnpm_semver_include_prerelease::is_valid_semver_range;

/// Whether `mode` rewrites `(alias, spec)` to a selector outside the range
/// the group records.
pub(super) fn moves_past_declared_range(
    mode: GlobalUpdateMode<'_>,
    alias: &str,
    spec: &str,
) -> bool {
    match mode {
        GlobalUpdateMode::DeclaredRange => false,
        GlobalUpdateMode::Latest => latest_selector(alias, spec).is_some(),
        GlobalUpdateMode::Tag(_) => is_registry_version_spec(spec),
    }
}

/// The selectors that reinstall a group. With `--latest`, plain version specs,
/// `npm:` aliases, and `runtime:` dependencies use `latest` so the seeded
/// manifest range cannot hold them back. With `--tag`, only version, range,
/// and dist-tag specs use the tag (see [`is_registry_version_spec`]). `pins`
/// holds back aliases that would otherwise move backwards.
pub(super) fn update_selectors(
    dependencies: &[(String, String)],
    mode: GlobalUpdateMode<'_>,
    pins: &HashMap<String, String>,
) -> Vec<String> {
    let selectors = dependencies
        .iter()
        .map(|(alias, spec)| match mode {
            GlobalUpdateMode::DeclaredRange => format!("{alias}@{spec}"),
            GlobalUpdateMode::Latest => {
                latest_selector(alias, spec).unwrap_or_else(|| format!("{alias}@{spec}"))
            }
            GlobalUpdateMode::Tag(tag) if is_registry_version_spec(spec) => {
                format!("{alias}@{tag}")
            }
            GlobalUpdateMode::Tag(_) => format!("{alias}@{spec}"),
        })
        .collect();
    apply_pins(dependencies, selectors, pins)
}

/// `selectors` with every pinned alias held at its pin instead.
pub(super) fn apply_pins(
    dependencies: &[(String, String)],
    selectors: Vec<String>,
    pins: &HashMap<String, String>,
) -> Vec<String> {
    dependencies
        .iter()
        .zip(selectors)
        .map(|((alias, spec), selector)| match pins.get(alias) {
            Some(pin) => pinned_selector(alias, spec, pin),
            None => selector,
        })
        .collect()
}

/// A version, range, or dist-tag: the only specs `--tag` replaces. Every
/// other form, including a git shorthand (`user/repo`) or a relative path,
/// names a source the tag may not exist on.
pub(super) fn is_registry_version_spec(spec: &str) -> bool {
    get_version_selector_type(spec).is_some()
}

pub(super) fn needs_runtime_range_bump(
    dependencies: &[(String, String)],
    pins: &HashMap<String, String>,
) -> bool {
    dependencies
        .iter()
        .any(|(alias, spec)| {
            !pins.contains_key(alias) && declared_runtime_range(alias, spec).is_some()
        })
}

pub(super) fn apply_runtime_range_bumps(
    dependencies: &[(String, String)],
    selectors: &[String],
    pins: &HashMap<String, String>,
    resolved: &HashMap<String, Version>,
    range_spec_style: RangeSpecStyle,
) -> Vec<String> {
    dependencies
        .iter()
        .zip(selectors)
        .map(|((alias, spec), selector)| {
            if pins.contains_key(alias) {
                return selector.clone();
            }
            resolved
                .get(alias)
                .and_then(|ver| bumped_runtime_latest_selector(alias, spec, ver, range_spec_style))
                .unwrap_or_else(|| selector.clone())
        })
        .collect()
}

/// The warning `update -g` prints for a group it skips because the `file:`
/// source one of its dependencies was installed from is gone. Reinstalling the
/// group would fail with `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND`, and the group
/// still works from what it installed, so the other groups are updated instead.
pub(super) fn missing_file_source_warning(pkg: &GlobalPackageInfo) -> Option<String> {
    let (alias, source) = pkg.dependencies
        .iter()
        .find_map(|(alias, spec)| {
            let source = local_file_path(spec, &pkg.install_dir)?;
            matches!(source.try_exists(), Ok(false)).then_some((alias, source))
        })?;
    Some(format!(
        r#"Skipped updating {} because "{}" no longer exists. Reinstall {alias} from an existing location, or remove it with "pnpm remove -g {alias}"."#,
        pkg.aliases().join(", "),
        source.display(),
    ))
}

fn latest_selector(alias: &str, spec: &str) -> Option<String> {
    if !spec.contains(':') {
        return Some(format!("{alias}@latest"));
    }
    if let Some(rest) = spec.strip_prefix("npm:") {
        return latest_npm_alias_selector(alias, rest);
    }
    pnpm_tool_protocol::strip_tool_protocol(spec)
        .and_then(|runtime_spec| latest_runtime_selector(alias, runtime_spec))
}

fn latest_npm_alias_selector(alias: &str, rest: &str) -> Option<String> {
    let (target_pkg, version_spec) = parse_npm_target(alias, rest)?;
    version_spec
        .as_deref()
        .is_none_or(|inner| !inner.contains(':'))
        .then(|| format!("{alias}@npm:{target_pkg}@latest"))
}

fn parse_npm_target(alias: &str, rest: &str) -> Option<(String, Option<String>)> {
    let parsed = parse_npm_spec_body(rest)?;
    Some((parsed.alias.unwrap_or_else(|| alias.to_string()), parsed.version_spec))
}

fn latest_runtime_selector(alias: &str, runtime_spec: &str) -> Option<String> {
    if !pnpm_package_manifest::is_runtime_alias(alias) {
        return None;
    }
    if alias != "node" {
        return Some(format!("{alias}@runtime:latest"));
    }
    let parsed = pnpm_engine_runtime_node_resolver::parse_node_specifier(runtime_spec).ok()?;
    Some(if parsed.release_channel == "release" {
        "node@runtime:latest".to_string()
    } else {
        format!("node@runtime:{}/latest", parsed.release_channel)
    })
}

fn pinned_selector(alias: &str, spec: &str, pin: &str) -> String {
    if !spec.contains(':') {
        return format!("{alias}@{pin}");
    }
    if let Some(rest) = spec.strip_prefix("npm:") {
        let target_pkg =
            parse_npm_target(alias, rest).map(|(pkg, _)| pkg).unwrap_or_else(|| alias.to_string());
        return format!("{alias}@npm:{target_pkg}@{pin}");
    }
    if let Some(runtime_spec) = pnpm_tool_protocol::strip_tool_protocol(spec) {
        return pinned_runtime_selector(alias, runtime_spec, pin);
    }
    format!("{alias}@{pin}")
}

fn pinned_runtime_selector(alias: &str, runtime_spec: &str, pin: &str) -> String {
    if alias == "node"
        && let Ok(parsed) = pnpm_engine_runtime_node_resolver::parse_node_specifier(runtime_spec)
        && parsed.release_channel != "release"
        && pnpm_engine_runtime_node_resolver::parse_node_specifier(pin)
            .is_ok_and(|pin_parsed| pin_parsed.release_channel == "release")
    {
        return format!("node@runtime:{}/{pin}", parsed.release_channel);
    }
    format!("{alias}@runtime:{pin}")
}

fn bumped_runtime_latest_selector(
    alias: &str,
    spec: &str,
    resolved: &Version,
    range_spec_style: RangeSpecStyle,
) -> Option<String> {
    let declared_range = declared_runtime_range(alias, spec)?;
    let range = calc_version_range(resolved, Some(declared_range), None, range_spec_style);
    Some(format!("{alias}@runtime:{range}"))
}

fn declared_runtime_range<'a>(alias: &str, spec: &'a str) -> Option<&'a str> {
    if alias == "node" || !pnpm_package_manifest::is_runtime_alias(alias) {
        return None;
    }
    let declared_range = pnpm_tool_protocol::strip_tool_protocol(spec)?.trim();
    (!declared_range.is_empty() && is_valid_semver_range(declared_range)).then_some(declared_range)
}
