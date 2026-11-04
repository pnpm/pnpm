use crate::{
    cli_args::add::AddRequest,
    package_specifier::{EcosystemPackageSpecifier, PackageSpecifierPlan},
};
use miette::{Result, bail};
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
    pub(crate) bare_patterns: Vec<String>,
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
    !selectors.is_empty()
        && selectors
            .iter()
            .all(|selector| selector.starts_with('!'))
}

/// A negated bare pattern already selects the rest through the bare matcher.
pub(super) fn updates_all_except_exclusions(
    selectors: &[String],
    parsed: &ParsedUpdateSelectors,
) -> bool {
    excludes_only(selectors) && parsed.bare_patterns.is_empty()
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
    selector
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("pkg:"))
}

pub(crate) fn patterns_for_ecosystem(
    packages: &[String],
    kind: EcosystemFilterKind,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for raw in packages {
        push_selector_pattern(raw, kind, &mut out)?;
    }
    Ok(out)
}

fn push_selector_pattern(
    raw: &str,
    kind: EcosystemFilterKind,
    out: &mut Vec<String>,
) -> Result<()> {
    let (negated, raw) = raw
        .strip_prefix('!')
        .map_or((false, raw), |raw| (true, raw));
    if let Some(rest) = raw.strip_prefix("crate:") {
        return push_prefixed_pattern(rest, kind, EcosystemFilterKind::Cargo, negated, out);
    }
    if let Some(rest) = raw.strip_prefix("pypi:") {
        return push_prefixed_pattern(rest, kind, EcosystemFilterKind::Python, negated, out);
    }
    if is_purl(raw) {
        return push_negated_purl_pattern(raw, kind, negated, out);
    }
    out.push(format_pattern(negated, raw));
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
fn normalize_python_name_pattern(pattern: &str) -> String {
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
    validate_ecosystem_selectors(packages, config)?;
    let mut parsed = ParsedUpdateSelectors::default();
    for raw in packages {
        let (selector, mode) = raw
            .strip_prefix('!')
            .map_or((raw.as_str(), SelectorMode::Include), |selector| {
                (selector, SelectorMode::Exclude)
            });
        if is_cargo_prefixed(selector) || is_python_prefixed(selector) || is_purl(selector) {
            parse_prefixed_update_selector(selector, mode, &mut parsed)?;
        } else {
            parse_bare_update_selector(raw, &mut parsed);
        }
    }
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
    let plan = PackageSpecifierPlan::parse([AddRequest::from(raw)])?;
    for node in plan.node_packages {
        push_node_update_selector(node.selector(), mode, parsed);
    }
    for eco in plan.ecosystem_packages {
        push_ecosystem_update_selector(eco, mode, parsed)?;
    }
    Ok(())
}

fn push_node_update_selector(
    selector: &str,
    mode: SelectorMode,
    parsed: &mut ParsedUpdateSelectors,
) {
    let selector = selector.to_string();
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
    let parsed_req = pnpm_python_resolver::parse_requirement(req)?;
    match mode {
        SelectorMode::Include => {
            let version_spec = parsed_req.version_or_url.as_ref().map(ToString::to_string);
            parsed.python_requests.push(PythonUpdateSelector {
                name: parsed_req.name.to_string(),
                version_spec,
                explicit: true,
            });
        }
        SelectorMode::Exclude => parsed.python_exclusions.push(parsed_req.name.to_string()),
    }
    Ok(())
}

fn parse_bare_update_selector(raw: &str, parsed: &mut ParsedUpdateSelectors) {
    parsed.node_selectors.push(raw.to_string());
    if raw.starts_with('@') {
        return;
    }
    if raw.contains('*') || raw.starts_with('!') {
        parsed.bare_patterns.push(raw.to_string());
        return;
    }
    let (name, spec) = raw
        .rsplit_once('@')
        .map_or((raw, None), |(name, ver)| (name, Some(ver.to_string())));
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
