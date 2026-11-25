use pnpm_config::{
    Config,
    version_policy::expand_package_version_spec,
};
use pnpm_deps_restorer::normalize_build_dep_path;
use pnpm_package_name::git_repo_allow_build_key_from_dep_path;
use pnpm_reporter::{
    Reporter,
    emit_global_warning,
};
use pnpm_resolving_deps_resolver::ResolvedTree;
use std::collections::HashSet;

pub(super) fn warn_unused_permissions<Sink: Reporter>(config: &Config, tree: &ResolvedTree) {
    if !config.warn_unused_permissions || config.workspace_permission_selectors.is_empty() {
        return;
    }
    let selectors: Vec<_> = config.workspace_permission_selectors
        .iter()
        .filter(|key| {
            config.allow_builds.contains_key(*key) || config.allow_skills.contains_key(*key)
        })
        .map(|selector| (selector, permission_keys(selector)))
        .collect();
    let mut unmatched: HashSet<&str> = selectors
        .iter()
        .flat_map(|(_, keys)| keys.iter().map(String::as_str))
        .collect();
    remove_resolved_keys(&mut unmatched, tree);
    let unused: Vec<_> = selectors
        .iter()
        .filter(|(_, keys)| {
            keys.iter()
                .all(|key| unmatched.contains(key.as_str()))
        })
        .map(|(selector, _)| permission_label(selector))
        .collect();
    if !unused.is_empty() {
        emit_global_warning::<Sink>(&format!(
            "These package permissions matched no dependency: {}. The permissions are kept for future installs.",
            unused.join(", "),
        ));
    }
}

fn permission_keys(selector: &str) -> Vec<String> {
    if selector.contains("||")
        && let Ok(keys) = expand_package_version_spec(selector)
    {
        return keys;
    }
    vec![normalize_build_dep_path(selector)]
}

fn remove_resolved_keys(unmatched: &mut HashSet<&str>, tree: &ResolvedTree) {
    for package in tree.packages.values() {
        if unmatched.is_empty() {
            break;
        }
        unmatched.remove(package.name().as_ref());
        unmatched.remove(format!("{}@{}", package.name(), package.version()).as_str());
        let key = normalize_build_dep_path(&package.id);
        unmatched.remove(key.as_str());
        if let Some(repo) = git_repo_allow_build_key_from_dep_path(&key) {
            unmatched.remove(repo.as_ref());
        }
    }
}

fn permission_label(selector: &str) -> String {
    if selector.contains("://") {
        return pnpm_package_name::parse_dep_path_name_version(selector)
            .map(|(name, _)| pnpm_network::redact_and_sanitize(name))
            .unwrap_or_else(|| pnpm_network::redact_url_for_display(selector));
    }
    pnpm_network::redact_and_sanitize(selector)
}
