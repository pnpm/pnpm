use std::collections::HashMap;

use pnpm_lockfile::{LockfileResolution, PackageKey, RegistryResolution};
use pnpm_resolving_npm_resolver::MINIMUM_RELEASE_AGE_VIOLATION_CODE;
use pnpm_resolving_resolver_base::ResolutionPolicyViolation;
use ssri::Integrity;

use super::{BlockedVersions, block_dead_end_parents, held_back_lines};

fn parent(name_ver: &str) -> PackageKey {
    name_ver.parse().expect("valid name@version")
}

fn violation(name: &str, version: &str, parents: &[&str]) -> ResolutionPolicyViolation {
    ResolutionPolicyViolation {
        name: name.parse().expect("valid package name"),
        version: version.to_string(),
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: Integrity::from(b"fixture".as_slice()),
            revision: None,
        }),
        code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
        reason: format!("{name}@{version} is too new"),
        parents_truncated: false,
        retry_parent: parents.last().map(|value| parent(value)),
        parents: parents
            .iter()
            .copied()
            .map(parent)
            .collect(),
    }
}

#[test]
fn blames_the_immediate_parent_of_each_immature_pick() {
    let mut blocked = BlockedVersions::new();
    let violations = [violation("binding", "1.2.5", &["vite@8.2.1", "rolldown@1.2.5"])];

    assert!(block_dead_end_parents(&violations, &mut blocked));

    // The version standing between the pick and an installable tree is the
    // one that declared the edge, not the importer's own dependency.
    assert_eq!(blocked.len(), 1);
    assert!(blocked["rolldown"].contains("1.2.5"));
}

#[test]
fn a_violation_without_a_retry_parent_does_not_hide_other_retry_choices() {
    for first_is_retryable in [false, true] {
        let mut blocked = BlockedVersions::new();
        let mut exotic = violation("binding", "1.2.5", &["wrapper@1.0.0"]);
        exotic.retry_parent = None;
        let mut violations = [exotic, violation("other", "1.0.0", &["parent@2.0.0"])];
        if first_is_retryable {
            violations.reverse();
        }
        assert!(block_dead_end_parents(&violations, &mut blocked));
        assert_eq!(blocked["parent"], std::collections::HashSet::from(["2.0.0".to_string()]));
    }
}

#[test]
fn reports_no_progress_once_every_parent_is_already_blocked() {
    let mut blocked = BlockedVersions::new();
    let violations = [violation("binding", "1.2.5", &["rolldown@1.2.5"])];

    assert!(block_dead_end_parents(&violations, &mut blocked));
    // The walk has run out of ancestors to move; retrying would loop.
    assert!(!block_dead_end_parents(&violations, &mut blocked));
}

#[test]
fn ignores_violations_from_other_policies() {
    let mut blocked = BlockedVersions::new();
    let mut other = violation("binding", "1.2.5", &["rolldown@1.2.5"]);
    other.code = "TRUST_DOWNGRADE";

    assert!(!block_dead_end_parents(&[other], &mut blocked));
    assert_eq!(blocked, HashMap::new());
}

#[test]
fn parent_blocks_preserve_registry_identity() {
    let mut blocked = BlockedVersions::new();
    let violations = [violation("binding", "1.2.5", &["rolldown@work:1.2.5"])];
    assert!(block_dead_end_parents(&violations, &mut blocked));
    assert_eq!(blocked["rolldown"], std::collections::HashSet::from(["work:1.2.5".to_string()]));
}

#[test]
fn held_back_reports_sort_package_names() {
    let blocked = HashMap::from([
        ("z-parent".to_string(), std::collections::HashSet::from(["2.0.0".to_string()])),
        ("a-parent".to_string(), std::collections::HashSet::from(["2.0.0".to_string()])),
    ]);
    assert_eq!(
        held_back_lines(&blocked, &std::collections::BTreeMap::new()),
        ["  a-parent@2.0.0", "  z-parent@2.0.0"],
    );
}

#[test]
fn parent_blocks_remove_peer_and_patch_suffixes() {
    let mut blocked = BlockedVersions::new();
    let violations =
        [violation("child", "1.0.0", &["parent@gh:1.2.3(peer@2.0.0)(patch_hash=abc)"])];
    assert!(block_dead_end_parents(&violations, &mut blocked));
    assert_eq!(blocked["parent"], std::collections::HashSet::from(["gh:1.2.3".to_string()]));
}

#[tokio::test]
async fn backtracking_preserves_other_policy_violations() {
    let mut calls = 0;
    let result = super::resolve_mature_dependency_tree::<pnpm_reporter::SilentReporter, _, _>(
        |blocked| {
            calls += 1;
            let mut trust = violation("other", "1.0.0", &[]);
            trust.code = "TRUST_DOWNGRADE";
            let mut violations = vec![trust];
            if blocked.is_none() {
                violations.push(violation("child", "2.0.0", &["parent@2.0.0"]));
            }
            std::future::ready(Ok(pnpm_resolving_deps_resolver::ResolveWorkspaceResult {
                merged_tree: pnpm_resolving_deps_resolver::ResolvedTree {
                    policy_violations: violations,
                    ..Default::default()
                },
                peers: pnpm_resolving_deps_resolver::WorkspaceResolvePeersResult::default(),
                time: std::collections::BTreeMap::new(),
            }))
        },
        std::path::Path::new("."),
        true,
    )
    .await
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(result.merged_tree.policy_violations.len(), 1);
    assert_eq!(result.merged_tree.policy_violations[0].code, "TRUST_DOWNGRADE");
}

#[test]
fn a_non_registry_immediate_parent_is_not_retried_from_its_diagnostic_label() {
    let mut issue = violation("child", "1.0.0", &["wrapper@1.0.0"]);
    issue.retry_parent = None;
    let mut blocked = BlockedVersions::new();
    assert!(!block_dead_end_parents(&[issue], &mut blocked));
    assert!(blocked.is_empty());
}
