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

pub(crate) fn is_cargo_prefixed(selector: &str) -> bool {
    selector.starts_with("crate:")
        || selector
            .get(..10)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("pkg:cargo/"))
}

pub(crate) fn is_python_prefixed(selector: &str) -> bool {
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
    if let Some(rest) = raw.strip_prefix("crate:") {
        if kind == EcosystemFilterKind::Cargo {
            out.push(strip_version_suffix(rest).to_string());
        }
        return Ok(());
    }
    if let Some(rest) = raw.strip_prefix("pypi:") {
        if kind == EcosystemFilterKind::Python {
            out.push(strip_version_suffix(rest).to_string());
        }
        return Ok(());
    }
    if is_purl(raw) {
        return push_purl_pattern(raw, kind, out);
    }
    out.push(raw.to_string());
    Ok(())
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
        if is_cargo_prefixed(raw) || is_python_prefixed(raw) || is_purl(raw) {
            parse_prefixed_update_selector(raw, &mut parsed)?;
        } else {
            parse_bare_update_selector(raw, &mut parsed);
        }
    }
    Ok(parsed)
}

fn parse_prefixed_update_selector(raw: &str, parsed: &mut ParsedUpdateSelectors) -> Result<()> {
    let plan = PackageSpecifierPlan::parse([AddRequest::from(raw)])?;
    for node in plan.node_packages {
        parsed.node_selectors.push(node.selector().to_string());
    }
    for eco in plan.ecosystem_packages {
        match eco {
            EcosystemPackageSpecifier::Cargo(spec) => {
                parsed.cargo_requests.push(CargoUpdateSelector {
                    name: spec.name,
                    version_spec: spec.version_spec,
                    explicit: true,
                });
            }
            EcosystemPackageSpecifier::Python(req) => {
                let parsed_req = pnpm_python_resolver::parse_requirement(&req)?;
                let version_spec = parsed_req.version_or_url.as_ref().map(ToString::to_string);
                parsed.python_requests.push(PythonUpdateSelector {
                    name: parsed_req.name.to_string(),
                    version_spec,
                    explicit: true,
                });
            }
            EcosystemPackageSpecifier::RustToolchain(_) => {}
        }
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
