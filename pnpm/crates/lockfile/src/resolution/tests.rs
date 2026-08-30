use super::{
    BinaryArchive, BinaryResolution, BinarySpec, DirectoryResolution, GitResolution,
    LockfileFormError, LockfileFormOptions, LockfileResolution, PlatformAssetResolution,
    PlatformAssetTarget, PlatformSelector, RegistryOptions, RegistryResolution, RegistryServerType,
    TarballResolution, TarballRevision, TarballUrlOptions, VariationsResolution,
    integrity_addressed_registry_tarball_url, is_git_hosted_tarball_url,
    is_integrity_addressed_registry_tarball_url, libc_matches, npm_tarball_url,
    registry_server_type, select_platform_variant,
};
use crate::serialize_yaml;
use pretty_assertions::assert_eq;
use ssri::Integrity;
use std::collections::BTreeMap;
use text_block_macros::text_block;

const GIT_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// No declared server type — the strict default every registry but
/// registry.npmjs.org is read with.
fn undeclared_form(registry: &str, include_tarball_url: bool) -> LockfileFormOptions<'_> {
    LockfileFormOptions { registry, server_type: None, include_tarball_url }
}

fn integrity(integrity_str: &str) -> Integrity {
    integrity_str.parse().expect("parse integrity string")
}

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

/// Render a resolution exactly as it appears under a `packages:` entry, then
/// dedent the `resolution:` block. Exercises the real write path: the deep key
/// sort and the single-line-vs-block decision both depend on the `resolution`
/// key and its enclosing `packages` context, so a bare top-level serialization
/// would not reflect what pnpm writes.
fn render_resolution(resolution: &LockfileResolution) -> String {
    let document = serde_json::json!({
        "packages": {
            "p@1.0.0": { "resolution": serde_json::to_value(resolution).unwrap() },
        },
    });
    serialize_yaml::to_string(&document)
        .unwrap()
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("resolution:"))
        .map(|line| line.strip_prefix("    ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn deserialize_tarball_resolution() {
    eprintln!("CASE: without integrity");
    let yaml = text_block! {
        "tarball: file:ts-pipe-compose-0.2.1.tgz"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "file:ts-pipe-compose-0.2.1.tgz".to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    });
    assert_eq!(received, expected);

    eprintln!("CASE: with integrity");
    let yaml = text_block! {
        "tarball: file:ts-pipe-compose-0.2.1.tgz"
        "integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "file:ts-pipe-compose-0.2.1.tgz".to_string(),
        integrity: integrity("sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==").into(),
        revision: None,
        git_hosted: None,
        path: None,
    });
    assert_eq!(received, expected);
}

#[test]
fn deserialize_tarball_resolution_with_git_hosted() {
    eprintln!("CASE: explicit gitHosted: true");
    let yaml = text_block! {
        "tarball: https://codeload.github.com/foo/bar/tar.gz/abc1234"
        "gitHosted: true"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
        integrity: None,
        revision: None,
        git_hosted: Some(true),
        path: None,
    });
    assert_eq!(received, expected);
}

/// The flag is a hint: the fetch dispatch and the store-index key
/// follow the URL, so a git-host archive URL counts as git-hosted even
/// when the lockfile says otherwise. A lockfile claiming `false` on
/// one would otherwise skip the prepare + packlist pass and install the
/// raw archive.
#[test]
fn is_git_hosted_follows_the_url_over_a_contradicting_flag() {
    let git_hosted_url = format!("https://codeload.github.com/foo/bar/tar.gz/{GIT_COMMIT}");

    for flag in [None, Some(false), Some(true)] {
        let resolution = TarballResolution {
            tarball: git_hosted_url.clone(),
            integrity: None,
            revision: None,
            git_hosted: flag,
            path: None,
        };
        assert!(resolution.is_git_hosted(), "a git-host archive URL is git-hosted, {flag:?}");
    }

    let plain = TarballResolution {
        tarball: "https://example.com/pkg-1.0.0.tgz".to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    };
    assert!(!plain.is_git_hosted());
}

#[test]
fn deserialize_tarball_resolution_backfills_git_hosted() {
    eprintln!("CASE: codeload.github.com");
    let yaml = format!("tarball: https://codeload.github.com/foo/bar/tar.gz/{GIT_COMMIT}");
    let received: LockfileResolution = serde_saphyr::from_str(&yaml).unwrap();
    dbg!(&received);
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("https://codeload.github.com/foo/bar/tar.gz/{GIT_COMMIT}"),
        integrity: None,
        revision: None,
        git_hosted: Some(true),
        path: None,
    });
    assert_eq!(received, expected);

    eprintln!("CASE: gitlab.com archive");
    let yaml = format!(
        "tarball: https://gitlab.com/foo/bar/-/archive/{GIT_COMMIT}/bar-{GIT_COMMIT}.tar.gz",
    );
    let received: LockfileResolution = serde_saphyr::from_str(&yaml).unwrap();
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: format!(
            "https://gitlab.com/foo/bar/-/archive/{GIT_COMMIT}/bar-{GIT_COMMIT}.tar.gz",
        ),
        integrity: None,
        revision: None,
        git_hosted: Some(true),
        path: None,
    });
    assert_eq!(received, expected);

    eprintln!("CASE: bitbucket.org archive");
    let yaml = format!("tarball: https://bitbucket.org/foo/bar/get/{GIT_COMMIT}.tar.gz");
    let received: LockfileResolution = serde_saphyr::from_str(&yaml).unwrap();
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("https://bitbucket.org/foo/bar/get/{GIT_COMMIT}.tar.gz"),
        integrity: None,
        revision: None,
        git_hosted: Some(true),
        path: None,
    });
    assert_eq!(received, expected);

    eprintln!("CASE: registry URL (must not back-fill)");
    let yaml = text_block! {
        "tarball: https://registry.npmjs.org/foo/-/foo-1.0.0.tgz"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz".to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    });
    assert_eq!(received, expected);

    eprintln!("CASE: github.com without tar.gz (must not back-fill)");
    let yaml = text_block! {
        "tarball: https://codeload.github.com/foo/bar/zip/abc1234"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/zip/abc1234".to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    });
    assert_eq!(received, expected);
}

#[test]
fn is_git_hosted_tarball_url_rejects_false_positives() {
    assert!(is_git_hosted_tarball_url(&format!(
        "https://codeload.github.com/foo/bar/tar.gz/{GIT_COMMIT}"
    )));
    assert!(is_git_hosted_tarball_url(&format!(
        "https://gitlab.com/api/v4/projects/foo%2Fbar/repository/archive.tar.gz?ref={GIT_COMMIT}"
    )));
    assert!(!is_git_hosted_tarball_url("https://gitlab.com/foo/bar?download=tar.gz"));
    assert!(!is_git_hosted_tarball_url("https://codeload.github.com/foo/bar/tar.gz/main"));
    assert!(!is_git_hosted_tarball_url(
        "https://gitlab.com/foo/bar/-/archive/main/bar-main.tar.gz",
    ));
    assert!(!is_git_hosted_tarball_url(
        "https://gitlab.com/api/v4/projects/foo%2Fbar/repository/archive.tar.gz",
    ));
    assert!(!is_git_hosted_tarball_url("https://bitbucket.org/foo/bar/get/main.tar.gz"));

    // Host lookalikes. The authority is compared whole, so neither a
    // `user@` prefix (where the real host is what follows the `@`) nor a
    // subdomain of a git provider passes for the provider itself — the
    // exemption from integrity checking rides on this.
    assert!(!is_git_hosted_tarball_url(&format!(
        "https://codeload.github.com@evil.example/foo/bar/tar.gz/{GIT_COMMIT}"
    )));
    assert!(!is_git_hosted_tarball_url(&format!(
        "https://sub.codeload.github.com/foo/bar/tar.gz/{GIT_COMMIT}"
    )));
    assert!(!is_git_hosted_tarball_url(&format!(
        "https://codeload.github.com.evil.example/foo/bar/tar.gz/{GIT_COMMIT}"
    )));
    assert!(!is_git_hosted_tarball_url(&format!(
        "https://gitlab.com@evil.example/api/v4/projects/foo%2Fbar/repository/archive.tar.gz?ref={GIT_COMMIT}"
    )));
    assert!(!is_git_hosted_tarball_url(&format!(
        "https://bitbucket.org@evil.example/foo/bar/get/{GIT_COMMIT}.tar.gz"
    )));
}

#[test]
fn serialize_tarball_resolution() {
    eprintln!("CASE: without integrity");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "file:ts-pipe-compose-0.2.1.tgz".to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {tarball: file:ts-pipe-compose-0.2.1.tgz}";
    assert_eq!(received, expected);

    eprintln!("CASE: with integrity");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "file:ts-pipe-compose-0.2.1.tgz".to_string(),
        integrity: integrity("sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==").into(),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==, tarball: file:ts-pipe-compose-0.2.1.tgz}";
    assert_eq!(received, expected);
}

#[test]
fn deserialize_tarball_resolution_with_path() {
    let yaml = text_block! {
        "tarball: https://codeload.github.com/foo/bar/tar.gz/abc1234"
        "gitHosted: true"
        "path: packages/sub"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
        integrity: None,
        revision: None,
        git_hosted: Some(true),
        path: Some("packages/sub".to_string()),
    });
    assert_eq!(received, expected);
}

#[test]
fn serialize_tarball_resolution_with_path() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
        integrity: None,
        revision: None,
        git_hosted: Some(true),
        path: Some("packages/sub".to_string()),
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {gitHosted: true, path: packages/sub, tarball: https://codeload.github.com/foo/bar/tar.gz/abc1234}";
    assert_eq!(received, expected);
}

#[test]
fn serialize_tarball_resolution_with_git_hosted() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
        integrity: integrity("sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==").into(),
        revision: None,
        git_hosted: Some(true),
        path: None,
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {gitHosted: true, integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==, tarball: https://codeload.github.com/foo/bar/tar.gz/abc1234}";
    assert_eq!(received, expected);
}

#[test]
fn deserialize_registry_resolution() {
    let yaml = text_block! {
        "integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let expected = LockfileResolution::Registry(RegistryResolution {
        integrity: integrity(
            "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==",
        ),
        revision: None,
    });
    assert_eq!(received, expected);
}

#[test]
fn serialize_registry_resolution() {
    let resolution = LockfileResolution::Registry(RegistryResolution {
        integrity: integrity(
            "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==",
        ),
        revision: None,
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==}";
    assert_eq!(received, expected);
}

#[test]
fn registry_revision_round_trips_in_the_compact_lockfile_form() {
    let resolution: LockfileResolution =
        serde_saphyr::from_str(&format!("integrity: {REVISION_SHA512}\nrevision: 2"))
            .expect("deserialize registry revision");
    let expected = LockfileResolution::Registry(RegistryResolution {
        integrity: integrity(REVISION_SHA512),
        revision: Some(TarballRevision::try_from(2).unwrap()),
    });
    assert_eq!(resolution, expected);
    assert_eq!(
        render_resolution(&resolution),
        format!("resolution: {{integrity: {REVISION_SHA512}, revision: 2}}"),
    );
}

#[test]
fn registry_revision_rejects_values_outside_the_positive_safe_integer_range() {
    for revision in ["0", "-1", "1.5", "9007199254740992", "'1'"] {
        let yaml = format!("integrity: {REVISION_SHA512}\nrevision: {revision}");
        let result = serde_saphyr::from_str::<LockfileResolution>(&yaml);
        assert!(result.is_err(), "revision {revision} must be rejected; got {result:?}");
    }
}

#[test]
fn deserialize_directory_resolution() {
    let yaml = text_block! {
        "type: directory"
        "directory: ts-pipe-compose-0.2.1/package"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let expected = LockfileResolution::Directory(DirectoryResolution {
        directory: "ts-pipe-compose-0.2.1/package".to_string(),
    });
    assert_eq!(received, expected);
}

#[test]
fn serialize_directory_resolution() {
    let resolution = LockfileResolution::Directory(DirectoryResolution {
        directory: "ts-pipe-compose-0.2.1/package".to_string(),
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {directory: ts-pipe-compose-0.2.1/package, type: directory}";
    assert_eq!(received, expected);
}

#[test]
fn deserialize_git_resolution() {
    let yaml = text_block! {
        "type: git"
        "repo: https://github.com/ksxnodemodules/ts-pipe-compose.git"
        "commit: e63c09e460269b0c535e4c34debf69bb91d57b22"
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
}

#[test]
fn deserialize_git_resolution_with_path() {
    let yaml = text_block! {
        "type: git"
        "repo: https://github.com/ksxnodemodules/ts-pipe-compose.git"
        "commit: e63c09e460269b0c535e4c34debf69bb91d57b22"
        "path: packages/sub"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    let expected = LockfileResolution::Git(GitResolution {
        repo: "https://github.com/ksxnodemodules/ts-pipe-compose.git".to_string(),
        commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
        integrity: None,
        path: Some("packages/sub".to_string()),
    });
    assert_eq!(received, expected);
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

#[test]
fn serialize_git_resolution() {
    let resolution = LockfileResolution::Git(GitResolution {
        repo: "https://github.com/ksxnodemodules/ts-pipe-compose.git".to_string(),
        commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
        integrity: None,
        path: None,
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {commit: e63c09e460269b0c535e4c34debf69bb91d57b22, repo: https://github.com/ksxnodemodules/ts-pipe-compose.git, type: git}";
    assert_eq!(received, expected);
}

#[test]
fn serialize_git_resolution_with_path() {
    let resolution = LockfileResolution::Git(GitResolution {
        repo: "https://github.com/ksxnodemodules/ts-pipe-compose.git".to_string(),
        commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
        integrity: None,
        path: Some("packages/sub".to_string()),
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {commit: e63c09e460269b0c535e4c34debf69bb91d57b22, path: packages/sub, repo: https://github.com/ksxnodemodules/ts-pipe-compose.git, type: git}";
    assert_eq!(received, expected);
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
fn deserialize_binary_resolution_tarball() {
    let yaml = text_block! {
        "type: binary"
        "url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz"
        "integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
        "bin: bin/node"
        "archive: tarball"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let expected = LockfileResolution::Binary(BinaryResolution {
        url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz".to_string(),
        integrity: integrity(
            "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==",
        ),
        bin: BinarySpec::Single("bin/node".to_string()),
        archive: BinaryArchive::Tarball,
        prefix: None,
    });
    assert_eq!(received, expected);
}

#[test]
fn deserialize_binary_resolution_zip_with_map_and_prefix() {
    let yaml = text_block! {
        "type: binary"
        "url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-win-x64.zip"
        "integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
        "bin:"
        "  node: node.exe"
        "archive: zip"
        "prefix: node-v22.0.0-win-x64"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let bin = BinarySpec::Map(BTreeMap::from([("node".to_string(), "node.exe".to_string())]));
    let expected = LockfileResolution::Binary(BinaryResolution {
        url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0-win-x64.zip".to_string(),
        integrity: integrity(
            "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==",
        ),
        bin,
        archive: BinaryArchive::Zip,
        prefix: Some("node-v22.0.0-win-x64".to_string()),
    });
    assert_eq!(received, expected);
}

#[test]
fn serialize_binary_resolution_tarball() {
    let resolution = LockfileResolution::Binary(BinaryResolution {
        url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz".to_string(),
        integrity: integrity(
            "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==",
        ),
        bin: BinarySpec::Single("bin/node".to_string()),
        archive: BinaryArchive::Tarball,
        prefix: None,
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "resolution:"
        "  archive: tarball"
        "  bin: bin/node"
        "  integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
        "  type: binary"
        "  url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz"
    };
    assert_eq!(received, expected);
}

#[test]
fn deserialize_variations_resolution() {
    let yaml = text_block! {
        "type: variations"
        "variants:"
        "  - resolution:"
        "      type: binary"
        "      url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz"
        "      integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
        "      bin: bin/node"
        "      archive: tarball"
        "    targets:"
        "      - os: darwin"
        "        cpu: arm64"
        "  - resolution:"
        "      type: binary"
        "      url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-linux-x64-musl.tar.gz"
        "      integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
        "      bin: bin/node"
        "      archive: tarball"
        "    targets:"
        "      - os: linux"
        "        cpu: x64"
        "        libc: musl"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let LockfileResolution::Variations(variations) = received else {
        panic!("expected Variations, got {received:?}");
    };
    assert_eq!(variations.variants.len(), 2);
    assert_eq!(variations.variants[0].targets.len(), 1);
    assert_eq!(variations.variants[0].targets[0].os, "darwin");
    assert_eq!(variations.variants[0].targets[0].cpu, "arm64");
    assert_eq!(variations.variants[0].targets[0].libc, None);
    assert_eq!(variations.variants[1].targets[0].libc.as_deref(), Some("musl"));
}

#[test]
fn serialize_variations_resolution() {
    let resolution = LockfileResolution::Variations(VariationsResolution {
        variants: vec![PlatformAssetResolution {
            resolution: LockfileResolution::Binary(BinaryResolution {
                url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz".to_string(),
                integrity: integrity(
                    "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==",
                ),
                bin: BinarySpec::Single("bin/node".to_string()),
                archive: BinaryArchive::Tarball,
                prefix: None,
            }),
            targets: vec![PlatformAssetTarget {
                os: "darwin".to_string(),
                cpu: "arm64".to_string(),
                libc: None,
            }],
        }],
    });
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "resolution:"
        "  type: variations"
        "  variants:"
        "    - resolution:"
        "        archive: tarball"
        "        bin: bin/node"
        "        integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
        "        type: binary"
        "        url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz"
        "      targets:"
        "        - cpu: arm64"
        "          os: darwin"
    };
    assert_eq!(received, expected);
}

// -----------------------------------------------------------------------------
// `select_platform_variant` / `libc_matches` — Slice B
// -----------------------------------------------------------------------------

fn binary_resolution(url: &str) -> LockfileResolution {
    LockfileResolution::Binary(BinaryResolution {
        url: url.to_string(),
        integrity: integrity(
            "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==",
        ),
        bin: BinarySpec::Single("bin/node".to_string()),
        archive: BinaryArchive::Tarball,
        prefix: None,
    })
}

fn target(os: &str, cpu: &str, libc: Option<&str>) -> PlatformAssetTarget {
    PlatformAssetTarget { os: os.to_string(), cpu: cpu.to_string(), libc: libc.map(str::to_string) }
}

fn variant(url: &str, targets: Vec<PlatformAssetTarget>) -> PlatformAssetResolution {
    PlatformAssetResolution { resolution: binary_resolution(url), targets }
}

fn selector(os: &str, cpu: &str, libc: Option<&str>) -> PlatformSelector {
    PlatformSelector { os: os.to_string(), cpu: cpu.to_string(), libc: libc.map(str::to_string) }
}

#[test]
fn pick_first_matching_variant() {
    let variants = vec![
        variant("darwin-arm64", vec![target("darwin", "arm64", None)]),
        variant("linux-x64", vec![target("linux", "x64", None)]),
    ];
    let picked = select_platform_variant(&variants, &selector("linux", "x64", Some("glibc")))
        .expect("matching variant");
    assert_eq!(
        picked.resolution.integrity().map(ToString::to_string),
        Some("sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==".to_string()),
        "picked variant should be the linux-x64 one (url is opaque to integrity, but the structural fixture means both share the same hash)",
    );
    assert_eq!(picked.targets, vec![target("linux", "x64", None)]);
}

#[test]
fn pick_matches_any_target_in_a_variant() {
    let variants = vec![variant(
        "darwin-universal",
        vec![target("darwin", "arm64", None), target("darwin", "x64", None)],
    )];
    let picked = select_platform_variant(&variants, &selector("darwin", "x64", None));
    assert!(picked.is_some());
}

#[test]
fn pick_returns_none_when_no_variant_matches() {
    let variants = vec![variant("darwin-arm64", vec![target("darwin", "arm64", None)])];
    assert!(select_platform_variant(&variants, &selector("linux", "x64", Some("glibc"))).is_none());
}

#[test]
fn pick_rejects_default_variant_for_musl_host() {
    let variants = vec![variant("linux-x64-glibc", vec![target("linux", "x64", None)])];
    assert!(
        select_platform_variant(&variants, &selector("linux", "x64", Some("musl"))).is_none(),
        "musl host must not silently pick the glibc default variant",
    );
}

#[test]
fn pick_returns_first_when_multiple_variants_match() {
    let variants = vec![
        variant("first-darwin-arm64", vec![target("darwin", "arm64", None)]),
        variant("second-darwin-arm64", vec![target("darwin", "arm64", None)]),
    ];
    let picked = select_platform_variant(&variants, &selector("darwin", "arm64", None))
        .expect("matching variant");
    let LockfileResolution::Binary(inner) = &picked.resolution else {
        panic!("expected Binary inner resolution");
    };
    assert_eq!(inner.url, "first-darwin-arm64", "declaration order must win");
}

#[test]
fn pick_matches_musl_variant_for_musl_host() {
    let variants = vec![
        variant("linux-x64-glibc", vec![target("linux", "x64", None)]),
        variant("linux-x64-musl", vec![target("linux", "x64", Some("musl"))]),
    ];
    let picked = select_platform_variant(&variants, &selector("linux", "x64", Some("musl")))
        .expect("musl variant present");
    assert_eq!(picked.targets, vec![target("linux", "x64", Some("musl"))]);
}

#[test]
fn libc_matches_truth_table() {
    assert!(libc_matches(None, None));
    assert!(!libc_matches(Some("musl"), None));
    assert!(!libc_matches(Some("glibc"), None));

    assert!(libc_matches(None, Some("glibc")));
    assert!(!libc_matches(Some("musl"), Some("glibc")));

    assert!(libc_matches(Some("musl"), Some("musl")));
    assert!(!libc_matches(None, Some("musl")));

    assert!(libc_matches(Some("uclibc"), Some("uclibc")));
    assert!(!libc_matches(None, Some("uclibc")));
    assert!(!libc_matches(Some("glibc"), Some("uclibc")));
}

const SHA512: &str = "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==";
const REVISION_SHA512: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

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
fn to_lockfile_form_rejects_a_revision_with_a_mismatched_url() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("https://attacker.example/-/tarballs/sha512/{}", "A".repeat(86)),
        integrity: Some(integrity(REVISION_SHA512)),
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
        Err(LockfileFormError::RevisionUrlMismatch { .. }),
    ));
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

/// A reconstructible registry tarball URL is dropped, leaving only the
/// integrity, so the path-preserving cases below are not just returning the
/// input unchanged.
#[test]
fn to_lockfile_form_drops_reconstructible_registry_tarball() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz".to_string(),
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let actual = resolution
        .to_lockfile_form("foo", "1.0.0", undeclared_form("https://registry.npmjs.org/", false))
        .unwrap();
    assert_eq!(
        actual,
        LockfileResolution::Registry(RegistryResolution {
            integrity: integrity(SHA512),
            revision: None
        }),
    );
}

/// The `path` selects the subdirectory to extract from a monorepo tarball
/// (`repo#commit&path:/sub/dir`). Dropping it makes later installs silently
/// unpack the repository root. See
/// <https://github.com/pnpm/pnpm/issues/12304>.
#[test]
fn to_lockfile_form_keeps_git_hosted_subdirectory_path() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: Some(true),
        path: Some("/packages/foo".to_string()),
    });
    let actual = resolution
        .to_lockfile_form("foo", "1.0.0", undeclared_form("https://registry.npmjs.org/", false))
        .unwrap();
    assert_eq!(actual, resolution);
}

/// `include_tarball_url` takes the same kept-URL branch, so it must keep
/// `path` too.
#[test]
fn to_lockfile_form_keeps_git_hosted_subdirectory_path_when_including_tarball_url() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: Some(true),
        path: Some("/packages/foo".to_string()),
    });
    let actual = resolution
        .to_lockfile_form("foo", "1.0.0", undeclared_form("https://registry.npmjs.org/", true))
        .unwrap();
    assert_eq!(actual, resolution);
}

#[test]
fn to_lockfile_form_keeps_scoped_tarball_with_percent_encoded_scope_separator() {
    for tarball_url in [
        "https://npm.example.com/@babel%2Fcore/-/core-7.0.0.tgz",
        "https://npm.example.com/@babel%2fcore/-/core-7.0.0.tgz",
    ] {
        let resolution = LockfileResolution::Tarball(TarballResolution {
            tarball: tarball_url.to_string(),
            integrity: Some(integrity(SHA512)),
            revision: None,
            git_hosted: None,
            path: None,
        });
        let actual = resolution
            .to_lockfile_form(
                "@babel/core",
                "7.0.0",
                undeclared_form("https://npm.example.com/", false),
            )
            .unwrap();
        assert_eq!(actual, resolution, "{tarball_url} must survive verbatim");
    }
}

#[test]
fn to_lockfile_form_drops_scoped_tarball_with_percent_encoding_on_the_public_registry() {
    for tarball_url in [
        "https://registry.npmjs.org/@babel%2Fcore/-/core-7.0.0.tgz",
        "https://registry.npmjs.org/@babel%2fcore/-/core-7.0.0.tgz",
    ] {
        let resolution = LockfileResolution::Tarball(TarballResolution {
            tarball: tarball_url.to_string(),
            integrity: Some(integrity(SHA512)),
            revision: None,
            git_hosted: None,
            path: None,
        });
        let actual = resolution
            .to_lockfile_form(
                "@babel/core",
                "7.0.0",
                undeclared_form("https://registry.npmjs.org/", false),
            )
            .unwrap();
        assert_eq!(
            actual,
            LockfileResolution::Registry(RegistryResolution {
                integrity: integrity(SHA512),
                revision: None
            }),
            "{tarball_url} must be dropped",
        );
    }
}

/// A URL that merely starts with the canonical URL but carries a trailing
/// `://suffix` is not canonical: stripping only the leading scheme keeps the
/// suffix, so it must not be dropped (the previous split-on-first-`://` logic
/// treated it as canonical).
#[test]
fn to_lockfile_form_keeps_tarball_with_trailing_scheme_separator() {
    let tarball = "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz://suffix".to_string();
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: tarball.clone(),
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let actual = resolution
        .to_lockfile_form("foo", "1.0.0", undeclared_form("https://registry.npmjs.org/", false))
        .unwrap();
    assert_eq!(
        actual,
        LockfileResolution::Tarball(TarballResolution {
            tarball,
            integrity: Some(integrity(SHA512)),
            revision: None,
            git_hosted: None,
            path: None,
        }),
    );
}

// --- Custom resolutions ---

fn custom_cdn_resolution() -> LockfileResolution {
    let mut extra = serde_json::Map::new();
    extra.insert(
        "url".to_string(),
        serde_json::Value::String("https://cdn.example.com/pkg.tgz".to_string()),
    );
    extra.insert("integrity".to_string(), serde_json::Value::String(SHA512.to_string()));
    LockfileResolution::Custom(super::CustomResolution {
        resolution_type: "custom:cdn".to_string().try_into().expect("custom type tag"),
        extra,
    })
}

#[test]
fn deserialize_custom_resolution_preserves_unknown_fields() {
    let yaml = text_block! {
        "type: custom:cdn"
        "url: https://cdn.example.com/pkg.tgz"
        "region: eu-west-1"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let LockfileResolution::Custom(custom) = &received else {
        panic!("expected a custom resolution, got {received:?}");
    };
    assert_eq!(custom.resolution_type.as_str(), "custom:cdn");
    assert_eq!(custom.extra["url"], "https://cdn.example.com/pkg.tgz");
    assert_eq!(custom.extra["region"], "eu-west-1");
}

#[test]
fn serialize_custom_resolution() {
    let received = render_resolution(&custom_cdn_resolution());
    eprintln!("RECEIVED:\n{received}");
    let expected = format!(
        "resolution: {{integrity: {SHA512}, type: custom:cdn, url: https://cdn.example.com/pkg.tgz}}",
    );
    assert_eq!(received, expected);
}

/// Custom resolutions enter pacquet as `serde_json::Value`s from a
/// pnpmfile custom resolver, so the JSON path must round-trip them
/// exactly (field set and `type` tag included).
#[test]
fn custom_resolution_round_trips_through_json() {
    let resolution = custom_cdn_resolution();
    let value = serde_json::to_value(&resolution).unwrap();
    dbg!(&value);
    assert_eq!(value["type"], "custom:cdn");
    let parsed: LockfileResolution = serde_json::from_value(value).unwrap();
    assert_eq!(parsed, resolution);
}

/// A malformed built-in resolution must stay a parse error: the custom
/// passthrough accepts only non-built-in `type` tags, so a `git` entry
/// missing its `commit` cannot silently reclassify as custom and dodge
/// the strict built-in shape checks.
#[test]
fn deserialize_rejects_malformed_builtin_resolution() {
    let yaml = text_block! {
        "type: git"
        "repo: https://github.com/user/repo.git"
    };
    let received = serde_saphyr::from_str::<LockfileResolution>(yaml);
    dbg!(&received);
    assert!(received.is_err(), "a git resolution without a commit must not parse");
}

const ARTIFACTORY_REGISTRY: &str = "https://artifactory.example/artifactory/api/npm/npm-virtual/";

/// The npm and Artifactory layouts differ only in a scoped package's filename.
#[test]
fn npm_tarball_url_keeps_the_scope_in_the_artifactory_filename() {
    for (name, version, expected) in [
        ("@acme/widget", "1.2.3", "@acme/widget/-/@acme/widget-1.2.3.tgz"),
        ("@acme/widget", "1.2.3+build.4", "@acme/widget/-/@acme/widget-1.2.3.tgz"),
        ("@acme/widget", "1.2.3-beta.1", "@acme/widget/-/@acme/widget-1.2.3-beta.1.tgz"),
        ("widget", "1.2.3", "widget/-/widget-1.2.3.tgz"),
    ] {
        let received = npm_tarball_url(
            name,
            version,
            TarballUrlOptions {
                registry: ARTIFACTORY_REGISTRY,
                server_type: Some(RegistryServerType::Artifactory),
            },
        );
        assert_eq!(received, format!("{ARTIFACTORY_REGISTRY}{expected}"));
    }
}

#[test]
fn npm_tarball_url_matches_the_npm_layout_for_an_unscoped_package() {
    for server_type in [None, Some(RegistryServerType::Npm), Some(RegistryServerType::Artifactory)]
    {
        let received = npm_tarball_url(
            "widget",
            "1.2.3",
            TarballUrlOptions { registry: ARTIFACTORY_REGISTRY, server_type },
        );
        assert_eq!(received, format!("{ARTIFACTORY_REGISTRY}widget/-/widget-1.2.3.tgz"));
    }
}

fn artifactory_form(include_tarball_url: bool) -> LockfileFormOptions<'static> {
    LockfileFormOptions {
        registry: ARTIFACTORY_REGISTRY,
        server_type: Some(RegistryServerType::Artifactory),
        include_tarball_url,
    }
}

#[test]
fn to_lockfile_form_drops_the_artifactory_url_of_a_scoped_package() {
    let tarball = format!("{ARTIFACTORY_REGISTRY}@acme/widget/-/@acme/widget-1.2.3.tgz");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let actual =
        resolution.to_lockfile_form("@acme/widget", "1.2.3", artifactory_form(false)).unwrap();
    assert_eq!(
        actual,
        LockfileResolution::Registry(RegistryResolution {
            integrity: integrity(SHA512),
            revision: None
        }),
    );
}

/// Under the Artifactory layout the npm-layout URL is the one pnpm cannot
/// rebuild, so it has to survive verbatim — the inverse of the default.
#[test]
fn to_lockfile_form_keeps_the_npm_layout_url_on_an_artifactory_registry() {
    let tarball = format!("{ARTIFACTORY_REGISTRY}@acme/widget/-/widget-1.2.3.tgz");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let actual =
        resolution.to_lockfile_form("@acme/widget", "1.2.3", artifactory_form(false)).unwrap();
    assert_eq!(actual, resolution);
}

#[test]
fn to_lockfile_form_keeps_the_artifactory_url_on_a_registry_left_on_the_npm_layout() {
    let tarball = format!("{ARTIFACTORY_REGISTRY}@acme/widget/-/@acme/widget-1.2.3.tgz");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let actual = resolution
        .to_lockfile_form("@acme/widget", "1.2.3", undeclared_form(ARTIFACTORY_REGISTRY, false))
        .unwrap();
    assert_eq!(actual, resolution);
}

#[test]
fn to_lockfile_form_keeps_the_artifactory_url_when_include_tarball_url_is_set() {
    let tarball = format!("{ARTIFACTORY_REGISTRY}@acme/widget/-/@acme/widget-1.2.3.tgz");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let actual =
        resolution.to_lockfile_form("@acme/widget", "1.2.3", artifactory_form(true)).unwrap();
    assert_eq!(actual, resolution);
}

#[test]
fn registry_server_type_is_undeclared_by_default_and_tolerates_a_missing_trailing_slash() {
    let options = BTreeMap::from([(
        ARTIFACTORY_REGISTRY.to_string(),
        RegistryOptions {
            server_type: Some(RegistryServerType::Artifactory),
            supports_time_field: None,
        },
    )]);
    assert_eq!(
        registry_server_type(&options, ARTIFACTORY_REGISTRY.trim_end_matches('/')),
        Some(RegistryServerType::Artifactory),
    );
    assert_eq!(registry_server_type(&options, "https://npm.example.com/"), None);
    assert_eq!(registry_server_type(&BTreeMap::new(), ARTIFACTORY_REGISTRY), None);
}

/// A registry declared to behave like registry.npmjs.org gets its leniency:
/// the percent-encoded scoped path is reconstructible there too. Undeclared,
/// the same URL must survive — the registry may serve only the encoded path.
/// See <https://github.com/pnpm/pnpm/issues/13534>.
#[test]
fn to_lockfile_form_drops_the_encoded_scoped_path_only_when_the_registry_is_declared_npm() {
    let registry = "https://npm.example.com/";
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{registry}@babel%2Fcore/-/core-7.0.0.tgz"),
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });

    let declared_npm = LockfileFormOptions {
        registry,
        server_type: Some(RegistryServerType::Npm),
        include_tarball_url: false,
    };
    assert_eq!(
        resolution.to_lockfile_form("@babel/core", "7.0.0", declared_npm).unwrap(),
        LockfileResolution::Registry(RegistryResolution {
            integrity: integrity(SHA512),
            revision: None
        }),
    );
    assert_eq!(
        resolution
            .to_lockfile_form("@babel/core", "7.0.0", undeclared_form(registry, false))
            .unwrap(),
        resolution,
    );
}
