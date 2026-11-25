use pnpm_config::Config;
use pnpm_deps_restorer::{
    AllowBuildPolicy,
    SkillsPolicy,
    normalize_build_dep_path,
};
use pnpm_package_name::{
    git_repo_allow_build_key_from_dep_path,
    parse_dep_path_name_version,
};
use pnpm_reporter::{
    Reporter,
    emit_global_warning,
};
use pnpm_resolving_deps_resolver::ResolvedTree;
use std::collections::HashMap;

pub(super) fn warn_unused_permissions<Sink: Reporter>(config: &Config, tree: &ResolvedTree) {
    if !config.warn_unused_permissions || config.workspace_permission_selectors.is_empty() {
        return;
    }
    let mut unmatched = HashMap::<String, Vec<PermissionRule>>::new();
    for selector in &config.workspace_permission_selectors {
        if let Some(rule) = PermissionRule::from_config(config, selector) {
            let key = normalize_build_dep_path(selector);
            unmatched
                .entry(permission_lookup_key(&key).to_owned())
                .or_default()
                .push(rule);
        }
    }
    remove_resolved_rules(&mut unmatched, tree);
    let mut unused: Vec<_> = unmatched
        .into_values()
        .flatten()
        .map(|rule| rule.selector)
        .collect();
    if unused.is_empty() {
        return;
    }
    unused.sort_unstable();
    let labels = unused
        .into_iter()
        .map(permission_label)
        .collect::<Vec<_>>();
    emit_global_warning::<Sink>(&format!(
        "These package permissions matched no dependency: {}. The permissions are kept for future installs.",
        labels.join(", "),
    ));
}

/// Check each selector independently so overlapping rules can both match.
struct PermissionRule<'a> {
    selector: &'a str,
    build: Option<AllowBuildPolicy>,
    skills: Option<SkillsPolicy>,
}

impl<'a> PermissionRule<'a> {
    fn from_config(config: &Config, selector: &'a str) -> Option<Self> {
        let build = config.allow_builds
            .get(selector)
            .and_then(|decision| {
                AllowBuildPolicy::from_approvals(&[(selector.to_owned(), *decision)].into(), false)
                    .ok()
            });
        let skills = config.allow_skills
            .get(selector)
            .and_then(|decision| {
                SkillsPolicy::from_approvals(&[(selector.to_owned(), decision.clone())].into()).ok()
            });
        (build.is_some() || skills.is_some()).then_some(Self { selector, build, skills })
    }

    fn matches(&self, path: &str) -> bool {
        self.build
            .as_ref()
            .and_then(|policy| policy.check(path))
            .is_some()
            || self.skills
                .as_ref()
                .and_then(|policy| policy.check(path))
                .is_some()
    }
}

fn permission_lookup_key(selector: &str) -> &str {
    parse_dep_path_name_version(selector).map(|(name, _)| name).unwrap_or(selector)
}

fn remove_resolved_rules(
    unmatched: &mut HashMap<String, Vec<PermissionRule>>,
    tree: &ResolvedTree,
) {
    for package in tree.packages.values() {
        if unmatched.is_empty() {
            break;
        }
        let path = if package.id.starts_with("link:") {
            format!("{}@{}", package.name(), package.id)
        } else {
            normalize_build_dep_path(&package.id)
        };
        let key = permission_lookup_key(&path);
        remove_matching_rules(unmatched, key, &path);
        if let Some(repo) = git_repo_allow_build_key_from_dep_path(&path)
            && permission_lookup_key(&repo) != key
        {
            remove_matching_rules(unmatched, permission_lookup_key(&repo), &path);
        }
    }
}

fn remove_matching_rules(
    unmatched: &mut HashMap<String, Vec<PermissionRule>>,
    key: &str,
    path: &str,
) {
    if let Some(rules) = unmatched.get_mut(key) {
        rules.retain(|rule| !rule.matches(path));
        if rules.is_empty() {
            unmatched.remove(key);
        }
    }
}

fn permission_label(selector: &str) -> String {
    if selector.starts_with("//") {
        return pnpm_network::redact_url_for_display(&format!("https:{selector}"));
    }
    if selector.contains("//") {
        return pnpm_package_name::parse_dep_path_name_version(selector)
            .filter(|(name, _)| pnpm_package_name::is_valid_dependency_alias(name))
            .map(|(name, _)| name.to_string())
            .unwrap_or_else(|| pnpm_network::redact_url_for_display(selector));
    }
    pnpm_network::redact_and_sanitize(selector)
}
