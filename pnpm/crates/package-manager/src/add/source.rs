use pnpm_reporter::{LogEvent, LogLevel, PnpmLog};
use pnpm_resolving_git_resolver::HostedGit;

pub(super) fn is_same_source(spec1: &str, spec2: &str, alias: &str) -> bool {
    if spec1 == spec2 {
        return true;
    }
    get_specifier_source(spec1, alias) == get_specifier_source(spec2, alias)
}

pub(super) fn collect_dependency_warnings(
    catalog_warning: Option<LogEvent>,
    prev_specifier: Option<&str>,
    package_selector: &str,
    package_name: &str,
    prefix: &str,
) -> Vec<LogEvent> {
    let mut warnings = Vec::new();
    if let Some(warning) = catalog_warning {
        warnings.push(warning);
    }
    if let Some(prev_spec) =
        prev_specifier.filter(|spec| !is_same_source(spec, package_selector, package_name))
    {
        warnings.push(LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: format!(
                r#"Replaced "{package_name}" ("{prev_spec}") with "{package_selector}" from a different source."#,
            ),
            prefix: prefix.to_string(),
        }));
    }
    warnings
}

fn get_specifier_source(spec: &str, alias: &str) -> String {
    if let Some(hosted) = HostedGit::from_url(spec) {
        return format!("git:{:?}:{}:{}", hosted.host_type, hosted.user, hosted.project);
    }
    if spec.starts_with("git+") || spec.starts_with("git:") || spec.ends_with(".git") {
        let repo_part = spec.split('#').next().unwrap_or(spec);
        return format!("git:{repo_part}");
    }
    let parsed = pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency(spec);
    if let Some(source) =
        parsed.bare_specifier.as_deref().and_then(|bare| get_bare_specifier_source(bare, alias))
    {
        return source;
    }
    format!("npm:{alias}")
}

fn get_bare_specifier_source(bare: &str, alias: &str) -> Option<String> {
    if let Some(aliased_name) = bare.strip_prefix("npm:") {
        let pkg_name = parse_aliased_npm_package_name(aliased_name);
        return Some(format!("npm:{pkg_name}"));
    }
    if bare.starts_with("file:") || bare.starts_with("link:") || bare.starts_with("portal:") {
        return Some(format!("file:{bare}"));
    }
    if bare.starts_with("workspace:") {
        return Some(format!("workspace:{alias}"));
    }
    if bare.starts_with("catalog:") {
        return Some(format!("catalog:{bare}"));
    }
    if bare.starts_with("http:") || bare.starts_with("https:") {
        let url_part = bare.split('#').next().unwrap_or(bare);
        return Some(format!("url:{url_part}"));
    }
    None
}

fn parse_aliased_npm_package_name(aliased_name: &str) -> &str {
    if let Some(rest) = aliased_name.strip_prefix('@') {
        if let Some(idx) = rest.find('@') { &aliased_name[..=idx] } else { aliased_name }
    } else if let Some(idx) = aliased_name.find('@') {
        &aliased_name[..idx]
    } else {
        aliased_name
    }
}
