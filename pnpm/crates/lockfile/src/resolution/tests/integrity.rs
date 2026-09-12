use super::{
    GitResolution, LockfileFormError, LockfileResolution, REVISION_SHA512, RegistryResolution,
    TarballResolution, TarballRevision, assert_eq, integrity,
    integrity_addressed_registry_tarball_url, is_integrity_addressed_registry_tarball_url,
    render_resolution, text_block, undeclared_form,
};

/// An `integrity: ''` entry — what an edited lockfile carries when the hash
/// is emptied instead of deleted — parses into an SRI with zero hashes. It
/// pins nothing, so `checkable_integrity` reports it as absent while the raw
/// accessor still shows what the lockfile said.
#[test]
fn empty_integrity_string_is_not_checkable() {
    let yaml = text_block! {
        "tarball: https://registry.example/p/-/p-1.0.0.tgz"
        "integrity: ''"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    assert!(received.integrity().is_some());
    assert!(received.checkable_integrity().is_none());
}

#[test]
fn deserialize_git_resolution_with_integrity() {
    let yaml = text_block! {
        "type: git"
        "repo: https://github.com/ksxnodemodules/ts-pipe-compose.git"
        "commit: e63c09e460269b0c535e4c34debf69bb91d57b22"
        "integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let expected = LockfileResolution::Git(GitResolution {
        repo: "https://github.com/ksxnodemodules/ts-pipe-compose.git".to_string(),
        commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
        integrity: None,
        path: None,
    });
    assert_eq!(received, expected);
    assert!(received.integrity().is_none());
}

/// The value is discarded, so a hash pnpm's own reader tolerates must not
/// become a parse error here.
#[test]
fn deserialize_git_resolution_with_a_malformed_integrity() {
    let yaml = text_block! {
        "type: git"
        "repo: https://github.com/ksxnodemodules/ts-pipe-compose.git"
        "commit: e63c09e460269b0c535e4c34debf69bb91d57b22"
        "integrity: not-a-real-hash"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let LockfileResolution::Git(git) = &received else { panic!("expected a git resolution") };
    assert_eq!(git.integrity, None, "the malformed hash must not survive the read");
}

/// Writing the hash back would keep advertising a check nothing performs,
/// so it leaves on the next write.
#[test]
fn serialize_git_resolution_drops_the_integrity() {
    let resolution = LockfileResolution::Git(GitResolution {
        repo: "https://github.com/ksxnodemodules/ts-pipe-compose.git".to_string(),
        commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
        integrity: Some(
            "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
                .to_string(),
        ),
        path: None,
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {commit: e63c09e460269b0c535e4c34debf69bb91d57b22, repo: https://github.com/ksxnodemodules/ts-pipe-compose.git, type: git}";
    assert_eq!(received, expected);
}

#[test]
fn integrity_addressed_tarball_url_is_relative_to_the_declared_registry() {
    let registry = "https://registry.example.test/npm/private";
    let expected = format!("{registry}/-/tarballs/sha512/{}", "A".repeat(86));
    assert_eq!(
        integrity_addressed_registry_tarball_url(&integrity(REVISION_SHA512), registry),
        Some(expected.clone()),
    );
    assert!(is_integrity_addressed_registry_tarball_url(
        &expected,
        &integrity(REVISION_SHA512),
        registry,
    ));
    assert!(!is_integrity_addressed_registry_tarball_url(
        &format!("{expected}?token=untrusted"),
        &integrity(REVISION_SHA512),
        registry,
    ));
    assert!(!is_integrity_addressed_registry_tarball_url(
        "https://registry.example.test/npm/private/foo/-/foo-1.0.0.tgz",
        &integrity(REVISION_SHA512),
        registry,
    ));
}

#[test]
fn to_lockfile_form_always_compacts_an_integrity_addressed_revision() {
    let registry = "https://registry.example.test/npm/private/";
    let tarball = integrity_addressed_registry_tarball_url(&integrity(REVISION_SHA512), registry)
        .expect("complete sha512 integrity");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity(REVISION_SHA512)),
        revision: Some(TarballRevision::try_from(3).unwrap()),
        git_hosted: None,
        path: None,
    });
    assert_eq!(
        resolution.to_lockfile_form("foo", "1.0.0", undeclared_form(registry, true)).unwrap(),
        LockfileResolution::Registry(RegistryResolution {
            integrity: integrity(REVISION_SHA512),
            revision: Some(TarballRevision::try_from(3).unwrap()),
        }),
    );
}

#[test]
fn to_lockfile_form_always_normalizes_an_integrity_addressed_url_without_a_revision() {
    let registry = "https://registry.example.test/npm/private/";
    let tarball = integrity_addressed_registry_tarball_url(&integrity(REVISION_SHA512), registry)
        .expect("complete sha512 integrity");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity(REVISION_SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });

    assert_eq!(
        resolution.to_lockfile_form("foo", "1.0.0", undeclared_form(registry, true)).unwrap(),
        LockfileResolution::Registry(RegistryResolution {
            integrity: integrity(REVISION_SHA512),
            revision: None,
        }),
    );
}

#[test]
fn to_lockfile_form_rejects_a_revision_without_integrity() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://registry.example.test/-/tarballs/sha512/digest".to_string(),
        integrity: None,
        revision: Some(TarballRevision::try_from(3).unwrap()),
        git_hosted: None,
        path: None,
    });

    assert!(matches!(
        resolution.to_lockfile_form(
            "foo",
            "1.0.0",
            undeclared_form("https://registry.example.test/", false),
        ),
        Err(LockfileFormError::RevisionWithoutIntegrity),
    ));
}
