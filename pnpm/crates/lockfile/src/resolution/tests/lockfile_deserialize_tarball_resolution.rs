use super::{
    BTreeMap, BinaryArchive, BinaryResolution, BinarySpec, DirectoryResolution, GIT_COMMIT,
    GitResolution, LockfileFormError, LockfileResolution, PlatformAssetResolution,
    PlatformAssetTarget, REVISION_SHA512, RegistryResolution, SHA512, TarballResolution,
    TarballRevision, VariationsResolution, assert_eq, custom_cdn_resolution, integrity,
    render_resolution, text_block, undeclared_form,
};

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
