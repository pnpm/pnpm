use crate::{
    cli_args::add::AddRequest,
    package_specifier::{
        EcosystemPackageSpecifier,
        PackageSpecifierPlan,
    },
};
use miette::{
    Result,
    bail,
};
use pnpm_config::Config;
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EcosystemFilterKind {
    Node,
    Cargo,
    Python,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ParsedUpdateSelectors {
    pub(crate) node_selectors: Vec<String>,
    pub(crate) cargo_requests: Vec<CargoUpdateSelector>,
    pub(crate) python_requests: Vec<PythonUpdateSelector>,
    pub(crate) cargo_exclusions: Vec<String>,
    pub(crate) python_exclusions: Vec<String>,
    pub(crate) cargo_patterns: Vec<String>,
    pub(crate) python_patterns: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CargoUpdateSelector {
    pub(crate) name: String,
    pub(crate) version_spec: Option<String>,
    pub(crate) explicit: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PythonUpdateSelector {
    pub(crate) name: String,
    pub(crate) version_spec: Option<String>,
    pub(crate) explicit: bool,
}

pub(crate) fn validate_ecosystem_selectors(selectors: &[String], config: &Config) -> Result<()> {
    for selector in selectors {
        if is_cargo_prefixed(selector) && !config.cargo.enabled {
            bail!("crate: dependencies require `cargo.enabled: true` in pnpm-workspace.yaml");
        }
        if is_python_prefixed(selector) && !config.python.enabled {
            bail!("pypi: dependencies require `python.enabled: true` in pnpm-workspace.yaml");
        }
    }
    Ok(())
}

/// Whether every selector is negated, so the update covers every dependency
/// of every ecosystem except the named ones.
pub(crate) fn excludes_only(selectors: &[String]) -> bool {
    !selectors.is_empty() && !pnpm_matcher::has_include_pattern(selectors)
}

/// A negated bare pattern already selects the rest through the bare matcher.
pub(super) fn updates_all_except_exclusions(
    selectors: &[String],
    parsed: &ParsedUpdateSelectors,
) -> bool {
    excludes_only(selectors) && parsed.cargo_patterns.is_empty()
}

pub(crate) fn is_cargo_prefixed(selector: &str) -> bool {
    let selector = selector.strip_prefix('!').unwrap_or(selector);
    selector.starts_with("crate:")
        || selector
            .get(..10)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("pkg:cargo/"))
}

pub(crate) fn is_python_prefixed(selector: &str) -> bool {
    let selector = selector.strip_prefix('!').unwrap_or(selector);
    selector.starts_with("pypi:")
        || selector
            .get(..9)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("pkg:pypi/"))
}

fn is_purl(selector: &str) -> bool {
    let selector = selector.strip_prefix('!').unwrap_or(selector);
    selector
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("pkg:"))
}

pub(crate) fn is_ecosystem_prefixed(selector: &str) -> bool {
    is_cargo_prefixed(selector) || is_python_prefixed(selector) || is_purl(selector)
}

pub(crate) fn patterns_for_ecosystem(
    packages: &[String],
    kind: EcosystemFilterKind,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for raw in packages {
        push_selector_pattern(raw, kind, &mut out)?;
    }
    Ok(pnpm_matcher::prune_orphan_ignores(out, packages))
}

fn push_selector_pattern(
    raw: &str,
    kind: EcosystemFilterKind,
    out: &mut Vec<String>,
) -> Result<()> {
    let (negated, raw) = match raw.strip_prefix('!') {
        Some(stripped) => (true, stripped),
        None => (false, raw),
    };
    if raw.starts_with('@') {
        match kind {
            EcosystemFilterKind::Node => out.push(format_pattern(negated, raw)),
            EcosystemFilterKind::Cargo | EcosystemFilterKind::Python => {}
        }
        return Ok(());
    }
    if let Some(rest) = raw.strip_prefix("crate:") {
        return push_prefixed_pattern(rest, kind, EcosystemFilterKind::Cargo, negated, out);
    }
    if let Some(rest) = raw.strip_prefix("pypi:") {
        return push_prefixed_pattern(rest, kind, EcosystemFilterKind::Python, negated, out);
    }
    if is_purl(raw) {
        return push_negated_purl_pattern(raw, kind, negated, out);
    }
    let pattern = if kind == EcosystemFilterKind::Python {
        normalize_python_name_pattern(raw)
    } else {
        raw.to_string()
    };
    out.push(format_pattern(negated, &pattern));
    Ok(())
}

fn push_prefixed_pattern(
    spec: &str,
    kind: EcosystemFilterKind,
    expected: EcosystemFilterKind,
    negated: bool,
    out: &mut Vec<String>,
) -> Result<()> {
    if kind == expected {
        let name = match expected {
            EcosystemFilterKind::Cargo => strip_version_suffix(spec).to_string(),
            EcosystemFilterKind::Python => python_pattern_name(spec)?,
            EcosystemFilterKind::Node => unreachable!(),
        };
        out.push(format_pattern(negated, &name));
    }
    Ok(())
}

fn python_pattern_name(spec: &str) -> Result<String> {
    let requirement = spec
        .rsplit_once('@')
        .filter(|(name, version)| {
            !name
                .chars()
                .last()
                .is_some_and(char::is_whitespace)
                && !version.contains('/')
                && !version.contains(':')
        })
        .map_or(spec, |(name, _)| name);
    match pnpm_python_resolver::parse_requirement(requirement) {
        Ok(parsed) => Ok(parsed.name.to_string()),
        Err(_) if requirement.contains(['*', '?']) => {
            Ok(normalize_python_name_pattern(requirement))
        }
        Err(error) => Err(error),
    }
}

/// Applies Python name normalization to the literal parts of a glob, so it
/// matches the normalized names of declared dependencies.
pub(super) fn normalize_python_name_pattern(pattern: &str) -> String {
    let mut normalized = String::with_capacity(pattern.len());
    for character in pattern.chars() {
        if matches!(character, '-' | '_' | '.') {
            if !normalized.ends_with('-') {
                normalized.push('-');
            }
        } else {
            normalized.push(character.to_ascii_lowercase());
        }
    }
    normalized
}

fn push_negated_purl_pattern(
    raw: &str,
    kind: EcosystemFilterKind,
    negated: bool,
    out: &mut Vec<String>,
) -> Result<()> {
    let start = out.len();
    push_purl_pattern(raw, kind, out)?;
    if negated {
        for pattern in &mut out[start..] {
            pattern.insert(0, '!');
        }
    }
    Ok(())
}

fn format_pattern(negated: bool, pattern: &str) -> String {
    if negated { format!("!{pattern}") } else { pattern.to_string() }
}

fn push_purl_pattern(raw: &str, kind: EcosystemFilterKind, out: &mut Vec<String>) -> Result<()> {
    let plan = PackageSpecifierPlan::parse([AddRequest::from(raw)])?;
    for node in plan.node_packages {
        if kind == EcosystemFilterKind::Node {
            let parsed = parse_wanted_dependency(node.selector());
            out.push(parsed.alias.unwrap_or_else(|| node.selector().to_string()));
        }
    }
    for eco in plan.ecosystem_packages {
        match (kind, eco) {
            (EcosystemFilterKind::Cargo, EcosystemPackageSpecifier::Cargo(spec)) => {
                out.push(spec.name);
            }
            (EcosystemFilterKind::Python, EcosystemPackageSpecifier::Python(req)) => {
                let parsed = pnpm_python_resolver::parse_requirement(&req)?;
                out.push(parsed.name.to_string());
            }
            _ => {}
        }
    }
    Ok(())
}

fn strip_version_suffix(spec: &str) -> &str {
    spec.rsplit_once('@').map_or(spec, |(name, _)| name)
}

pub(crate) fn parse_update_selectors(
    packages: &[String],
    config: &Config,
) -> Result<ParsedUpdateSelectors> {
    parse_update_selection(packages, config, BareUpdateNames::AllEcosystems)
}

#[derive(Clone, Copy)]
pub(crate) enum BareUpdateNames {
    AllEcosystems,
    NodeOnly,
}

pub(crate) fn parse_update_selection(
    packages: &[String],
    config: &Config,
    bare_names: BareUpdateNames,
) -> Result<ParsedUpdateSelectors> {
    validate_ecosystem_selectors(packages, config)?;
    let mut parsed = ParsedUpdateSelectors::default();
    for raw in packages {
        let (selector, mode) = match raw.strip_prefix('!') {
            Some(selector) => (selector, SelectorMode::Exclude),
            None => (raw.as_str(), SelectorMode::Include),
        };
        if is_cargo_prefixed(selector) || is_python_prefixed(selector) || is_purl(selector) {
            parse_prefixed_update_selector(selector, mode, &mut parsed)?;
        } else {
            match bare_names {
                BareUpdateNames::NodeOnly => parsed.node_selectors.push(raw.clone()),
                BareUpdateNames::AllEcosystems => parse_bare_update_selector(raw, &mut parsed),
            }
        }
    }
    parsed.node_selectors = pnpm_matcher::prune_orphan_ignores(parsed.node_selectors, packages);
    parsed.cargo_patterns = pnpm_matcher::prune_orphan_ignores(parsed.cargo_patterns, packages);
    parsed.python_patterns = pnpm_matcher::prune_orphan_ignores(parsed.python_patterns, packages);
    Ok(parsed)
}

#[derive(Clone, Copy)]
enum SelectorMode {
    Include,
    Exclude,
}

fn parse_prefixed_update_selector(
    raw: &str,
    mode: SelectorMode,
    parsed: &mut ParsedUpdateSelectors,
) -> Result<()> {
    if let Some(pattern) = raw.strip_prefix("pypi:").filter(|name| is_package_name_glob(name)) {
        let name = normalize_python_name_pattern(pattern);
        match mode {
            SelectorMode::Include => parsed.python_patterns.push(name),
            SelectorMode::Exclude => parsed.python_exclusions.push(name),
        }
        return Ok(());
    }
    if let Some(pattern) = raw.strip_prefix("crate:").filter(|name| is_package_name_glob(name)) {
        match mode {
            SelectorMode::Include => parsed.cargo_patterns.push(pattern.to_owned()),
            SelectorMode::Exclude => parsed.cargo_exclusions.push(pattern.to_owned()),
        }
        return Ok(());
    }
    let plan = PackageSpecifierPlan::parse([AddRequest::from(raw)])?;
    for node in plan.node_packages {
        push_node_update_selector(node.selector().to_owned(), mode, parsed);
    }
    for eco in plan.ecosystem_packages {
        push_ecosystem_update_selector(eco, mode, parsed)?;
    }
    Ok(())
}

fn is_package_name_glob(name: &str) -> bool {
    name.contains(['*', '?'])
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.*?".contains(&byte))
}

fn push_node_update_selector(
    selector: String,
    mode: SelectorMode,
    parsed: &mut ParsedUpdateSelectors,
) {
    parsed.node_selectors.push(match mode {
        SelectorMode::Include => selector,
        SelectorMode::Exclude => format!("!{selector}"),
    });
}

fn push_ecosystem_update_selector(
    selector: EcosystemPackageSpecifier,
    mode: SelectorMode,
    parsed: &mut ParsedUpdateSelectors,
) -> Result<()> {
    match selector {
        EcosystemPackageSpecifier::Cargo(spec) => {
            push_cargo_update_selector(spec.name, spec.version_spec, mode, parsed);
        }
        EcosystemPackageSpecifier::Python(req) => {
            push_python_update_selector(&req, mode, parsed)?;
        }
        EcosystemPackageSpecifier::RustToolchain(_) => {}
    }
    Ok(())
}

fn push_cargo_update_selector(
    name: String,
    version_spec: Option<String>,
    mode: SelectorMode,
    parsed: &mut ParsedUpdateSelectors,
) {
    match mode {
        SelectorMode::Include => {
            parsed.cargo_requests.push(CargoUpdateSelector { name, version_spec, explicit: true });
        }
        SelectorMode::Exclude => parsed.cargo_exclusions.push(name),
    }
}

fn push_python_update_selector(
    req: &str,
    mode: SelectorMode,
    parsed: &mut ParsedUpdateSelectors,
) -> Result<()> {
    match mode {
        SelectorMode::Exclude => {
            parsed.python_exclusions.push(python_pattern_name(req)?);
            return Ok(());
        }
        SelectorMode::Include => {}
    }
    let parsed_req = pnpm_python_resolver::parse_requirement(req)?;
    if matches!(parsed_req.version_or_url, Some(pnpm_python_installer::VersionOrUrl::Url(_))) {
        miette::bail!(
            "direct URL requirements are not supported by pnpm update; use pnpm add instead",
        );
    }
    let version_spec = parsed_req.version_or_url.as_ref().map(ToString::to_string);
    parsed.python_requests.push(PythonUpdateSelector {
        name: parsed_req.name.to_string(),
        version_spec,
        explicit: true,
    });
    Ok(())
}

fn parse_bare_update_selector(raw: &str, parsed: &mut ParsedUpdateSelectors) {
    parsed.node_selectors.push(raw.to_string());
    let unnegated = raw.strip_prefix('!').unwrap_or(raw);
    if unnegated.starts_with('@') {
        return;
    }
    if let Some(excluded) = raw.strip_prefix('!') {
        let bare_name = strip_version_suffix(excluded);
        parsed.cargo_exclusions.push(bare_name.to_string());
        if let Ok(py_name) = python_pattern_name(excluded) {
            parsed.python_exclusions.push(py_name);
        }
        parsed.cargo_patterns.push(raw.to_string());
        parsed.python_patterns.push(normalize_python_name_pattern(raw));
        return;
    }
    if raw.contains(['*', '?']) {
        parsed.cargo_patterns.push(raw.to_string());
        parsed.python_patterns.push(normalize_python_name_pattern(raw));
        return;
    }
    let (name, spec) = match raw.rsplit_once('@') {
        Some((name, ver)) => (name, Some(ver.to_string())),
        None => (raw, None),
    };
    parsed.cargo_requests.push(CargoUpdateSelector {
        name: name.to_string(),
        version_spec: spec.clone(),
        explicit: false,
    });
    parsed.python_requests.push(PythonUpdateSelector {
        name: name.to_string(),
        version_spec: spec,
        explicit: false,
    });
}

#[cfg(test)]
mod tests;
