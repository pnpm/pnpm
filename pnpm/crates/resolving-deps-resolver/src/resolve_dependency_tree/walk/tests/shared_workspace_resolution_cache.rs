use pnpm_resolving_resolver_base::{ResolveOptions, WantedDependency};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use crate::resolve_dependency_tree::workspace_ctx::WantedKey;

fn wanted(specifier: &str) -> WantedDependency {
    WantedDependency {
        alias: Some("shared".to_string()),
        bare_specifier: Some(specifier.to_string()),
        ..WantedDependency::default()
    }
}

fn key(wanted: &WantedDependency, project_dir: &str) -> WantedKey {
    (
        wanted.alias.clone(),
        wanted.bare_specifier.clone(),
        wanted.optional,
        wanted.injected,
        false,
        None,
        Some(PathBuf::from(project_dir)),
        None,
        Vec::new(),
        None,
        false,
    )
}

fn opts(project_dir: &str) -> ResolveOptions {
    ResolveOptions {
        project_dir: PathBuf::from(project_dir),
        lockfile_dir: PathBuf::from("/repo"),
        workspace_packages: Some(Arc::new(BTreeMap::new())),
        ..ResolveOptions::default()
    }
}

#[test]
fn shares_only_named_workspace_selectors_and_ignores_project_dir() {
    let base_wanted = wanted("workspace:^");
    let options = opts("/repo/packages/a");
    let shared = super::super::shared_workspace_key(
        &key(&base_wanted, "/repo/packages/a"),
        &base_wanted,
        &options,
    )
    .expect("named workspace selector is shareable");
    assert!(
        shared
            == super::super::shared_workspace_key(
                &key(&base_wanted, "/repo/apps/b"),
                &base_wanted,
                &ResolveOptions {
                    project_dir: PathBuf::from("/repo/apps/b"),
                    workspace_packages: options.workspace_packages.clone(),
                    ..options.clone()
                },
            )
            .expect("consumer-independent key"),
    );

    for specifier in ["^1.0.0", "link:../shared", "file:../shared", "workspace:./shared"] {
        let wanted = wanted(specifier);
        assert!(
            super::super::shared_workspace_key(
                &key(&wanted, "/repo/packages/a"),
                &wanted,
                &options,
            )
            .is_none(),
        );
    }
}

#[test]
fn separates_distinct_workspace_maps() {
    let wanted = wanted("workspace:^");
    let first = opts("/repo/packages/a");
    let second = opts("/repo/apps/b");
    assert!(
        super::super::shared_workspace_key(&key(&wanted, "/repo/packages/a"), &wanted, &first,)
            != super::super::shared_workspace_key(&key(&wanted, "/repo/apps/b"), &wanted, &second,),
    );
}
