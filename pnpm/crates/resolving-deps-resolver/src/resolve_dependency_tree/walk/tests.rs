mod shared_workspace_resolution_cache;

use pnpm_lockfile::{LockfileResolution, PkgNameVerPeer, RegistryResolution, TarballRevision};

use super::{
    exact_registry_specifier_for_revision_refresh, landed_on_prior_entry,
    registry_revisions_conflict,
};

fn key(raw: &str) -> PkgNameVerPeer {
    raw.parse().expect("parse snapshot key")
}

#[test]
fn matches_a_plain_registry_id() {
    assert!(landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.0.0"));
    assert!(!landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.1.0"));
}

#[test]
fn strips_the_recorded_key_peer_and_patch_suffixes() {
    assert!(landed_on_prior_entry(&key("foo@1.0.0(bar@2.0.0)"), "foo@1.0.0"));
    assert!(landed_on_prior_entry(&key("foo@1.0.0(patch_hash=0000)"), "foo@1.0.0"));
    assert!(landed_on_prior_entry(&key("foo@1.0.0(patch_hash=0000)(bar@2.0.0)"), "foo@1.0.0"));
}

#[test]
fn strips_the_resolved_id_patch_suffix() {
    assert!(landed_on_prior_entry(
        &key("foo@1.0.0(patch_hash=0000)"),
        "foo@1.0.0(patch_hash=0000)"
    ));
    assert!(landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.0.0(patch_hash=0000)"));
}

#[test]
fn matches_a_name_prefixed_file_id() {
    assert!(landed_on_prior_entry(&key("foo@file:packages/foo"), "foo@file:packages/foo"));
    assert!(!landed_on_prior_entry(&key("foo@file:packages/foo"), "file:packages/foo"));
}

fn revision_resolution(revision: u64, integrity: &str) -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution {
        integrity: integrity.parse().unwrap(),
        revision: Some(TarballRevision::try_from(revision).unwrap()),
    })
}

#[test]
fn conflicting_registry_revisions_are_detected() {
    assert!(registry_revisions_conflict(
        &revision_resolution(
            1,
            "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="
        ),
        &revision_resolution(
            2,
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=="
        ),
    ));
}

#[test]
fn identical_registry_revisions_can_share_an_identity() {
    let resolution = revision_resolution(
        1,
        "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
    );
    assert!(!registry_revisions_conflict(&resolution, &resolution));
}

#[test]
fn revision_refresh_pins_registry_specifiers_without_overriding_revision_selectors() {
    assert_eq!(exact_registry_specifier_for_revision_refresh("^1.0.0", "1.2.3", None), "1.2.3");
    assert_eq!(
        exact_registry_specifier_for_revision_refresh("npm:@scope/pkg@^1.0.0", "1.2.3", None,),
        "npm:@scope/pkg@1.2.3",
    );
    assert_eq!(
        exact_registry_specifier_for_revision_refresh(
            "corp:@scope/pkg@^1.0.0",
            "1.2.3",
            Some("corp"),
        ),
        "corp:@scope/pkg@1.2.3",
    );
    assert_eq!(
        exact_registry_specifier_for_revision_refresh("corp:@scope/pkg", "1.2.3", Some("corp"),),
        "corp:@scope/pkg@1.2.3",
    );
    assert_eq!(
        exact_registry_specifier_for_revision_refresh("corp:pkg", "1.2.3", Some("corp"),),
        "corp:pkg@1.2.3",
    );
    assert_eq!(
        exact_registry_specifier_for_revision_refresh("jsr:@scope/pkg", "1.2.3", None,),
        "jsr:@scope/pkg@1.2.3",
    );
    assert_eq!(
        exact_registry_specifier_for_revision_refresh("1.2.3+r1", "1.2.3", None),
        "1.2.3+r1",
    );
    assert_eq!(exact_registry_specifier_for_revision_refresh("^1.2.3+r1", "1.2.3", None), "1.2.3");
    assert_eq!(
        exact_registry_specifier_for_revision_refresh(
            "git+https://example.test/repo.git",
            "1.2.3",
            None
        ),
        "git+https://example.test/repo.git",
    );
}

/// A resolver that hands back no manifest still has to give the package
/// an identity — see <https://github.com/pnpm/pnpm/issues/13410>.
mod fallback_manifest {
    use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
    use pnpm_resolving_resolver_base::{CurrentPkg, PkgResolutionId, WantedDependency};

    fn wanted(alias: Option<&str>, bare_specifier: Option<&str>) -> WantedDependency {
        WantedDependency {
            alias: alias.map(str::to_string),
            bare_specifier: bare_specifier.map(str::to_string),
            ..WantedDependency::default()
        }
    }

    fn current_pkg(name: Option<&str>, version: Option<&str>) -> CurrentPkg {
        CurrentPkg {
            id: PkgResolutionId::from("file:sub"),
            name: name.map(str::to_string),
            version: version.map(str::to_string),
            resolution: LockfileResolution::Directory(DirectoryResolution {
                directory: "sub".to_string(),
            }),
            published_at: None,
        }
    }

    #[test]
    fn the_alias_names_the_package() {
        assert_eq!(
            super::super::fallback_manifest(
                &wanted(Some("no-manifest"), Some("file:./no-manifest-1.0.0.tgz")),
                None,
            ),
            serde_json::json!({ "name": "no-manifest", "version": "0.0.0" }),
        );
    }

    #[test]
    fn an_unaliased_dep_is_named_by_its_specifier_s_last_segment() {
        assert_eq!(
            super::super::fallback_manifest(
                &wanted(None, Some("https://example.com/no-manifest-1.0.0.tgz")),
                None,
            ),
            serde_json::json!({ "name": "no-manifest-1.0.0.tgz", "version": "0.0.0" }),
        );
    }

    #[test]
    fn the_lockfile_s_pin_wins_over_the_alias() {
        assert_eq!(
            super::super::fallback_manifest(
                &wanted(Some("sub"), Some("file:./sub")),
                Some(&current_pkg(Some("sub"), Some("2.0.0"))),
            ),
            serde_json::json!({ "name": "sub", "version": "2.0.0" }),
        );
    }

    #[test]
    fn a_half_recorded_pin_falls_through_to_the_alias() {
        assert_eq!(
            super::super::fallback_manifest(
                &wanted(Some("sub"), Some("file:./sub")),
                Some(&current_pkg(Some("sub"), None)),
            ),
            serde_json::json!({ "name": "sub", "version": "0.0.0" }),
        );
    }
}
