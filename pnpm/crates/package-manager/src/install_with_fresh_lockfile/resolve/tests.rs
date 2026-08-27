use std::collections::HashMap;

use pnpm_lockfile::{LockfileResolution, PkgNameVer, RegistryResolution};
use pnpm_resolving_npm_resolver::MINIMUM_RELEASE_AGE_VIOLATION_CODE;
use pnpm_resolving_resolver_base::ResolutionPolicyViolation;
use ssri::Integrity;

use super::{BlockedVersions, block_dead_end_parents};

fn parent(name_ver: &str) -> PkgNameVer {
    name_ver.parse().expect("valid name@version")
}

fn violation(name: &str, version: &str, parents: &[&str]) -> ResolutionPolicyViolation {
    ResolutionPolicyViolation {
        name: name.parse().expect("valid package name"),
        version: version.to_string(),
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
                .parse::<Integrity>()
                .expect("valid integrity"),
            revision: None,
        }),
        code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
        reason: format!("{name}@{version} is too new"),
        parents: parents.iter().copied().map(parent).collect(),
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
fn refuses_to_retry_when_a_violation_has_no_parent_to_blame() {
    let mut blocked = BlockedVersions::new();
    // The importer named this package itself. No ancestor's choice can widen
    // a range the manifest fixes, so another pass would re-reach this failure.
    let violations =
        [violation("binding", "1.2.5", &["rolldown@1.2.5"]), violation("is-odd", "0.1.2", &[])];

    assert!(!block_dead_end_parents(&violations, &mut blocked));
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
