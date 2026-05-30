use super::{
    BinaryArchive, BinaryResolution, BinarySpec, DirectoryResolution, GitResolution,
    LockfileResolution, PlatformAssetResolution, PlatformAssetTarget, PlatformSelector,
    RegistryResolution, TarballResolution, VariationsResolution, libc_matches,
    select_platform_variant,
};
use crate::serialize_yaml;
use pretty_assertions::assert_eq;
use ssri::Integrity;
use std::collections::BTreeMap;
use text_block_macros::text_block;

fn integrity(integrity_str: &str) -> Integrity {
    integrity_str.parse().expect("parse integrity string")
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
        git_hosted: Some(true),
        path: None,
    });
    assert_eq!(received, expected);
}

#[test]
fn deserialize_tarball_resolution_backfills_git_hosted() {
    // Lockfiles written by older pnpm versions don't carry `gitHosted`; the
    // loader back-fills it for entries whose URL matches a known git host.
    // Mirrors upstream's `enrichGitHostedFlag`.
    eprintln!("CASE: codeload.github.com");
    let yaml = text_block! {
        "tarball: https://codeload.github.com/foo/bar/tar.gz/abc1234"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    dbg!(&received);
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
        integrity: None,
        git_hosted: Some(true),
        path: None,
    });
    assert_eq!(received, expected);

    eprintln!("CASE: gitlab.com archive");
    let yaml = text_block! {
        "tarball: https://gitlab.com/foo/bar/-/archive/abc1234/bar-abc1234.tar.gz"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://gitlab.com/foo/bar/-/archive/abc1234/bar-abc1234.tar.gz".to_string(),
        integrity: None,
        git_hosted: Some(true),
        path: None,
    });
    assert_eq!(received, expected);

    eprintln!("CASE: bitbucket.org archive");
    let yaml = text_block! {
        "tarball: https://bitbucket.org/foo/bar/get/abc1234.tar.gz"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://bitbucket.org/foo/bar/get/abc1234.tar.gz".to_string(),
        integrity: None,
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
        git_hosted: None,
        path: None,
    });
    assert_eq!(received, expected);

    eprintln!("CASE: github.com without tar.gz (must not back-fill)");
    // Upstream's prefix check requires both the host prefix *and* a `tar.gz`
    // substring — release pages aren't tarballs.
    let yaml = text_block! {
        "tarball: https://codeload.github.com/foo/bar/zip/abc1234"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/zip/abc1234".to_string(),
        integrity: None,
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
        git_hosted: None,
        path: None,
    });
    let received = serialize_yaml::to_string(&resolution).unwrap();
    let received = received.trim();
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "tarball: file:ts-pipe-compose-0.2.1.tgz"
    };
    assert_eq!(received, expected);

    eprintln!("CASE: with integrity");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "file:ts-pipe-compose-0.2.1.tgz".to_string(),
        integrity: integrity("sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==").into(),
        git_hosted: None,
        path: None,
    });
    let received = serialize_yaml::to_string(&resolution).unwrap();
    let received = received.trim();
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "tarball: file:ts-pipe-compose-0.2.1.tgz"
        "integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
    };
    assert_eq!(received, expected);
}

#[test]
fn deserialize_tarball_resolution_with_path() {
    // Git-hosted tarballs from monorepos carry an optional sub-path
    // that points at the directory to pack. Mirrors pnpm's
    // `TarballResolution.path`.
    let yaml = text_block! {
        "tarball: https://codeload.github.com/foo/bar/tar.gz/abc1234"
        "gitHosted: true"
        "path: packages/sub"
    };
    let received: LockfileResolution = serde_saphyr::from_str(yaml).unwrap();
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
        integrity: None,
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
        git_hosted: Some(true),
        path: Some("packages/sub".to_string()),
    });
    let received = serialize_yaml::to_string(&resolution).unwrap();
    let received = received.trim();
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "tarball: https://codeload.github.com/foo/bar/tar.gz/abc1234"
        "gitHosted: true"
        "path: packages/sub"
    };
    assert_eq!(received, expected);
}

#[test]
fn serialize_tarball_resolution_with_git_hosted() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
        integrity: integrity("sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==").into(),
        git_hosted: Some(true),
        path: None,
    });
    let received = serialize_yaml::to_string(&resolution).unwrap();
    let received = received.trim();
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "tarball: https://codeload.github.com/foo/bar/tar.gz/abc1234"
        "integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
        "gitHosted: true"
    };
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
    });
    assert_eq!(received, expected);
}

#[test]
fn serialize_registry_resolution() {
    let resolution = LockfileResolution::Registry(RegistryResolution {
        integrity: integrity(
            "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==",
        ),
    });
    let received = serialize_yaml::to_string(&resolution).unwrap();
    let received = received.trim();
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
    };
    assert_eq!(received, expected);
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
    let received = serialize_yaml::to_string(&resolution).unwrap();
    let received = received.trim();
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "type: directory"
        "directory: ts-pipe-compose-0.2.1/package"
    };
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
        path: Some("packages/sub".to_string()),
    });
    assert_eq!(received, expected);
}

#[test]
fn serialize_git_resolution() {
    let resolution = LockfileResolution::Git(GitResolution {
        repo: "https://github.com/ksxnodemodules/ts-pipe-compose.git".to_string(),
        commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
        path: None,
    });
    let received = serialize_yaml::to_string(&resolution).unwrap();
    let received = received.trim();
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "type: git"
        "repo: https://github.com/ksxnodemodules/ts-pipe-compose.git"
        "commit: e63c09e460269b0c535e4c34debf69bb91d57b22"
    };
    assert_eq!(received, expected);
}

#[test]
fn serialize_git_resolution_with_path() {
    let resolution = LockfileResolution::Git(GitResolution {
        repo: "https://github.com/ksxnodemodules/ts-pipe-compose.git".to_string(),
        commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
        path: Some("packages/sub".to_string()),
    });
    let received = serialize_yaml::to_string(&resolution).unwrap();
    let received = received.trim();
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "type: git"
        "repo: https://github.com/ksxnodemodules/ts-pipe-compose.git"
        "commit: e63c09e460269b0c535e4c34debf69bb91d57b22"
        "path: packages/sub"
    };
    assert_eq!(received, expected);
}

/// A `BinaryResolution` for a tarball-shape runtime (Linux / macOS
/// Node), `bin` as a single string, no `prefix`. Mirrors what pnpm's
/// node-resolver writes for the `.tar.gz` branch.
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

/// A `BinaryResolution` for a zip-shape runtime (Windows Node), `bin`
/// as a name map, and `prefix` carrying the archive's top-level
/// directory name. Mirrors what pnpm's node-resolver writes for the
/// `.zip` branch at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/engine/runtime/node-resolver/src/index.ts>.
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

/// Round-trip a `BinaryResolution` so the serialized form pacquet
/// emits matches the shape upstream's resolver writes. Guards the
/// field ordering (and `prefix` omission for the tarball case) so a
/// lockfile pacquet round-trips stays diff-stable against pnpm.
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
    let received = serialize_yaml::to_string(&resolution).unwrap();
    let received = received.trim();
    eprintln!("RECEIVED:\n{received}");
    // Field order follows the struct declaration; `prefix: None`
    // skips serialization.
    let expected = text_block! {
        "type: binary"
        "url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz"
        "integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
        "bin: bin/node"
        "archive: tarball"
    };
    assert_eq!(received, expected);
}

/// A `VariationsResolution` wrapping two `BinaryResolution` variants
/// — the typical Node runtime shape: one variant per `(os, cpu)`
/// pair. `libc` only set on the linux-musl variant (when present);
/// omitted everywhere else.
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

/// Round-trip a single-variant `VariationsResolution`. Pinning the
/// emitted shape so the variant list, the inner resolution
/// discriminator, and the targets array all serialise in the same
/// order pnpm writes.
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
    let received = serialize_yaml::to_string(&resolution).unwrap();
    let received = received.trim();
    eprintln!("RECEIVED:\n{received}");
    let expected = text_block! {
        "type: variations"
        "variants:"
        "- resolution:"
        "    type: binary"
        "    url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz"
        "    integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
        "    bin: bin/node"
        "    archive: tarball"
        "  targets:"
        "  - os: darwin"
        "    cpu: arm64"
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

/// The picker returns the first variant whose `targets[]` contains an
/// `(os, cpu, libc)` triple matching the selector. Mirrors upstream's
/// declaration-order semantics — `Array.prototype.find` in
/// `selectPlatformVariant`.
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

/// One variant can cover multiple host triples; the picker matches
/// against any entry in the variant's `targets[]`. Real-world Node
/// archives ship a single `darwin` tarball that covers both `x64`
/// and `arm64` via separate target entries.
#[test]
fn pick_matches_any_target_in_a_variant() {
    let variants = vec![variant(
        "darwin-universal",
        vec![target("darwin", "arm64", None), target("darwin", "x64", None)],
    )];
    let picked = select_platform_variant(&variants, &selector("darwin", "x64", None));
    assert!(picked.is_some());
}

/// No variant matching the host triple → `None`. The install
/// dispatcher will surface this as a typed "no variant matches host
/// platform" error (Slice D).
#[test]
fn pick_returns_none_when_no_variant_matches() {
    let variants = vec![variant("darwin-arm64", vec![target("darwin", "arm64", None)])];
    assert!(select_platform_variant(&variants, &selector("linux", "x64", Some("glibc"))).is_none());
}

/// On a musl host, the glibc-default variant must NOT win silently.
/// Upstream rejects a `None`-libc variant when the selector requests
/// `musl`, requiring an exact `libc: "musl"` annotation to match.
/// Without this, a musl host would attempt to run a glibc-linked
/// binary.
#[test]
fn pick_rejects_default_variant_for_musl_host() {
    let variants = vec![variant("linux-x64-glibc", vec![target("linux", "x64", None)])];
    assert!(
        select_platform_variant(&variants, &selector("linux", "x64", Some("musl"))).is_none(),
        "musl host must not silently pick the glibc default variant",
    );
}

/// When two variants both match the same `(os, cpu, libc)` triple,
/// declaration order wins — mirroring upstream's `Array.prototype.find`
/// in `selectPlatformVariant`. Pinning this guards against a future
/// refactor that reorders the iteration (e.g., to a `BTreeMap` keyed
/// by triple) since pnpm-written lockfiles can rely on the order
/// (e.g., listing a preferred build before a fallback).
#[test]
fn pick_returns_first_when_multiple_variants_match() {
    // Both variants list the same darwin-arm64 target. The first one
    // is identified by its URL via the inner `BinaryResolution`.
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

/// A musl variant is picked only when the selector requests musl.
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

/// `libc_matches` truth table. Pinning each cell guards the
/// upstream contract in
/// <https://github.com/pnpm/pnpm/blob/94240bc046/resolving/resolver-base/src/index.ts#L100-L107>:
/// `None`-libc selector or `"glibc"` selector → variant libc must be
/// `None`; any other selector value → exact match.
#[test]
fn libc_matches_truth_table() {
    // Selector says "no libc constraint" (non-Linux host): only
    // the default (unannotated) variant matches.
    assert!(libc_matches(None, None));
    assert!(!libc_matches(Some("musl"), None));
    assert!(!libc_matches(Some("glibc"), None));

    // Selector says "glibc" (Linux glibc host): same rule as None.
    assert!(libc_matches(None, Some("glibc")));
    assert!(!libc_matches(Some("musl"), Some("glibc")));

    // Selector says "musl" (Linux musl host): require exact musl
    // annotation; the default variant is rejected.
    assert!(libc_matches(Some("musl"), Some("musl")));
    assert!(!libc_matches(None, Some("musl")));

    // Selector says an unknown libc (future-compat): require
    // exact match. The default variant is rejected so a future
    // libc value can't be silently aliased to glibc.
    assert!(libc_matches(Some("uclibc"), Some("uclibc")));
    assert!(!libc_matches(None, Some("uclibc")));
    assert!(!libc_matches(Some("glibc"), Some("uclibc")));
}
