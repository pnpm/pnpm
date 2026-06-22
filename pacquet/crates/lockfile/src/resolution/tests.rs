use super::{
    BinaryArchive, BinaryResolution, BinarySpec, DirectoryResolution, GitResolution,
    LockfileResolution, PlatformAssetResolution, PlatformAssetTarget, PlatformSelector,
    RegistryResolution, TarballResolution, VariationsResolution, is_git_hosted_tarball_url,
    libc_matches, select_platform_variant,
};
use crate::serialize_yaml;
use pretty_assertions::assert_eq;
use ssri::Integrity;
use std::collections::BTreeMap;
use text_block_macros::text_block;

const GIT_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn integrity(integrity_str: &str) -> Integrity {
    integrity_str.parse().expect("parse integrity string")
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
    eprintln!("CASE: codeload.github.com");
    let yaml = format!("tarball: https://codeload.github.com/foo/bar/tar.gz/{GIT_COMMIT}");
    let received: LockfileResolution = serde_saphyr::from_str(&yaml).unwrap();
    dbg!(&received);
    let expected = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("https://codeload.github.com/foo/bar/tar.gz/{GIT_COMMIT}"),
        integrity: None,
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
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {tarball: file:ts-pipe-compose-0.2.1.tgz}";
    assert_eq!(received, expected);

    eprintln!("CASE: with integrity");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "file:ts-pipe-compose-0.2.1.tgz".to_string(),
        integrity: integrity("sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==").into(),
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
    let received = render_resolution(&resolution);
    eprintln!("RECEIVED:\n{received}");
    let expected = "resolution: {integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==}";
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

/// A reconstructible registry tarball URL is dropped, leaving only the
/// integrity, so the path-preserving cases below are not just returning the
/// input unchanged.
#[test]
fn to_lockfile_form_drops_reconstructible_registry_tarball() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz".to_string(),
        integrity: Some(integrity(SHA512)),
        git_hosted: None,
        path: None,
    });
    let actual = resolution.to_lockfile_form("foo", "1.0.0", "https://registry.npmjs.org/", false);
    assert_eq!(
        actual,
        LockfileResolution::Registry(RegistryResolution { integrity: integrity(SHA512) }),
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
        git_hosted: Some(true),
        path: Some("/packages/foo".to_string()),
    });
    let actual = resolution.to_lockfile_form("foo", "1.0.0", "https://registry.npmjs.org/", false);
    assert_eq!(actual, resolution);
}

/// `include_tarball_url` takes the same kept-URL branch, so it must keep
/// `path` too.
#[test]
fn to_lockfile_form_keeps_git_hosted_subdirectory_path_when_including_tarball_url() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://codeload.github.com/foo/bar/tar.gz/abc1234".to_string(),
        integrity: Some(integrity(SHA512)),
        git_hosted: Some(true),
        path: Some("/packages/foo".to_string()),
    });
    let actual = resolution.to_lockfile_form("foo", "1.0.0", "https://registry.npmjs.org/", true);
    assert_eq!(actual, resolution);
}

/// Percent-encoding is case-insensitive, so a scoped tarball using uppercase
/// `%2F` is still the canonical URL and must be dropped just like `%2f`.
#[test]
fn to_lockfile_form_drops_scoped_tarball_with_uppercase_percent_encoding() {
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://registry.npmjs.org/@babel%2Fcore/-/core-7.0.0.tgz".to_string(),
        integrity: Some(integrity(SHA512)),
        git_hosted: None,
        path: None,
    });
    let actual =
        resolution.to_lockfile_form("@babel/core", "7.0.0", "https://registry.npmjs.org/", false);
    assert_eq!(
        actual,
        LockfileResolution::Registry(RegistryResolution { integrity: integrity(SHA512) }),
    );
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
        git_hosted: None,
        path: None,
    });
    let actual = resolution.to_lockfile_form("foo", "1.0.0", "https://registry.npmjs.org/", false);
    assert_eq!(
        actual,
        LockfileResolution::Tarball(TarballResolution {
            tarball,
            integrity: Some(integrity(SHA512)),
            git_hosted: None,
            path: None,
        }),
    );
}
