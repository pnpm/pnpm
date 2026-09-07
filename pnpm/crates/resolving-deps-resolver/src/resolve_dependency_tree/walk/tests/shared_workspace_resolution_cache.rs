use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use pnpm_resolving_resolver_base::{
    PkgResolutionId, ResolveOptions, ResolveResult, WantedDependency,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use super::super::{canonical_workspace_resolution, render_workspace_resolution};
use crate::resolve_dependency_tree::{TreeCtx, workspace_ctx::WantedKey};

fn wanted(specifier: &str) -> WantedDependency {
    WantedDependency {
        alias: Some("shared".to_string()),
        bare_specifier: Some(specifier.to_string()),
        ..WantedDependency::default()
    }
}

fn key(wanted: &WantedDependency, project_dir: &str) -> WantedKey {
    WantedKey::new((
        wanted.alias.clone(),
        wanted.bare_specifier.clone(),
        wanted.optional,
        wanted.injected,
        false,
        None,
        Some(PathBuf::from(project_dir).into()),
        None,
        Vec::new(),
        None,
        false,
    ))
}

fn opts(project_dir: &str) -> ResolveOptions {
    ResolveOptions {
        project_dir: PathBuf::from(project_dir),
        lockfile_dir: PathBuf::from("/repo"),
        workspace_packages: Some(Arc::new(BTreeMap::new())),
        ..ResolveOptions::default()
    }
}

/// A directory resolution shaped like the npm resolver's workspace output:
/// the id repeats the recorded directory behind its protocol prefix.
fn directory_result(id: &str, resolved_via: &str) -> ResolveResult {
    let directory =
        id.split_once(':').map_or_else(|| id.to_string(), |(_, directory)| directory.to_string());
    ResolveResult {
        id: PkgResolutionId::from(id.to_string()),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: None,
        resolution: LockfileResolution::Directory(DirectoryResolution { directory }),
        resolved_via: resolved_via.to_string(),
        normalized_bare_specifier: None,
        alias: Some("shared".to_string()),
        policy_violation: None,
    }
}

fn rendered_link(canonical: &ResolveResult, project_dir: &str) -> String {
    render_workspace_resolution(
        canonical,
        &crate::link_target::ImporterAnchor::new(Path::new(project_dir), Path::new("/repo")),
        Path::new(project_dir),
        Path::new("/repo"),
    )
    .id
    .as_str()
    .to_string()
}

#[test]
fn shares_only_named_workspace_selectors_and_ignores_project_dir() {
    let base_wanted = wanted("workspace:^");
    let options = opts("/repo/packages/a");
    let ctx = TreeCtx::new(options.clone());
    let shared = super::super::shared_workspace_key(
        &ctx,
        &key(&base_wanted, "/repo/packages/a"),
        &base_wanted,
        &options,
    )
    .expect("named workspace selector is shareable");
    let other_options =
        ResolveOptions { project_dir: PathBuf::from("/repo/apps/b"), ..options.clone() };
    let other_ctx = TreeCtx::new(other_options.clone());
    assert_eq!(
        shared,
        super::super::shared_workspace_key(
            &other_ctx,
            &key(&base_wanted, "/repo/apps/b"),
            &base_wanted,
            &other_options,
        )
        .expect("consumer-independent key"),
    );

    for specifier in ["^1.0.0", "link:../shared", "file:../shared", "workspace:./shared"] {
        let wanted = wanted(specifier);
        assert_eq!(
            super::super::shared_workspace_key(
                &ctx,
                &key(&wanted, "/repo/packages/a"),
                &wanted,
                &options
            ),
            None,
            "{specifier} must stay scoped to its consumer",
        );
    }
}

#[test]
fn separates_distinct_workspace_maps() {
    let wanted = wanted("workspace:^");
    let first = opts("/repo/packages/a");
    let second = opts("/repo/apps/b");
    let first_ctx = TreeCtx::new(first.clone());
    let second_ctx = TreeCtx::new(second.clone());
    assert_ne!(
        super::super::shared_workspace_key(
            &first_ctx,
            &key(&wanted, "/repo/packages/a"),
            &wanted,
            &first
        ),
        super::super::shared_workspace_key(
            &second_ctx,
            &key(&wanted, "/repo/apps/b"),
            &wanted,
            &second
        ),
    );
}

#[test]
fn link_resolution_round_trips_through_the_lockfile_root() {
    let resolved_for_a = directory_result("link:../shared", "workspace");
    let canonical = canonical_workspace_resolution(
        &resolved_for_a,
        Path::new("/repo/packages/a"),
        Path::new("/repo"),
    )
    .expect("a workspace link canonicalises against the lockfile root");
    assert_eq!(canonical.id.as_str(), "link:packages/shared");

    assert_eq!(rendered_link(&canonical, "/repo/packages/a"), resolved_for_a.id.as_str());
    assert_eq!(rendered_link(&canonical, "/repo/apps/nested/b"), "link:../../../packages/shared");
    assert_eq!(rendered_link(&canonical, "/repo"), "link:packages/shared");
    // A package that depends on itself keeps the bare `link:` the npm
    // resolver renders for an empty relative path.
    assert_eq!(rendered_link(&canonical, "/repo/packages/shared"), "link:");
}

#[test]
fn injected_resolution_is_already_consumer_independent() {
    let injected = directory_result("file:packages/shared", "workspace");
    let canonical = canonical_workspace_resolution(
        &injected,
        Path::new("/repo/packages/a"),
        Path::new("/repo"),
    )
    .expect("an injected workspace package is shareable as resolved");
    assert_eq!(canonical, injected);
    assert_eq!(rendered_link(&canonical, "/repo/apps/nested/b"), injected.id.as_str());
}

#[test]
fn rejects_resolutions_that_are_not_shareable_workspace_links() {
    let project_dir = Path::new("/repo/packages/a");
    let lockfile_dir = Path::new("/repo");
    // A local `link:` claimed by the local resolver, not the workspace one.
    assert_eq!(
        canonical_workspace_resolution(
            &directory_result("link:../shared", "local-directory"),
            project_dir,
            lockfile_dir,
        ),
        None,
    );
    // An id that does not restate the recorded directory: the round trip
    // through the lockfile root would not reproduce it.
    let mut mismatched = directory_result("link:../shared", "workspace");
    mismatched.id = PkgResolutionId::from("link:../other".to_string());
    assert_eq!(canonical_workspace_resolution(&mismatched, project_dir, lockfile_dir), None);
}
