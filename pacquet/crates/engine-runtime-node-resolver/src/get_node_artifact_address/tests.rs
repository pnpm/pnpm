use pretty_assertions::assert_eq;

use super::{GetNodeArtifactAddressOptions, NodeArtifactAddress, get_node_artifact_address};

/// Mirrors the
/// [`getNodeArtifactAddress`](https://github.com/pnpm/pnpm/blob/1627943d2a/engine/runtime/node-resolver/test/getNodeArtifactAddress.test.ts)
/// table-driven upstream test.
#[test]
fn matches_upstream_address_table() {
    let cases: &[(&str, &str, &str, &str, NodeArtifactAddress)] = &[
        (
            "16.0.0",
            "https://nodejs.org/download/release/",
            "win32",
            "ia32",
            NodeArtifactAddress {
                basename: "node-v16.0.0-win-x86".to_string(),
                dirname: "https://nodejs.org/download/release/v16.0.0".to_string(),
                extname: ".zip".to_string(),
            },
        ),
        (
            "16.0.0",
            "https://nodejs.org/download/release/",
            "linux",
            "arm",
            NodeArtifactAddress {
                basename: "node-v16.0.0-linux-armv7l".to_string(),
                dirname: "https://nodejs.org/download/release/v16.0.0".to_string(),
                extname: ".tar.gz".to_string(),
            },
        ),
        (
            "16.0.0",
            "https://nodejs.org/download/release/",
            "linux",
            "x64",
            NodeArtifactAddress {
                basename: "node-v16.0.0-linux-x64".to_string(),
                dirname: "https://nodejs.org/download/release/v16.0.0".to_string(),
                extname: ".tar.gz".to_string(),
            },
        ),
        (
            "15.14.0",
            "https://nodejs.org/download/release/",
            "darwin",
            "arm64",
            NodeArtifactAddress {
                basename: "node-v15.14.0-darwin-x64".to_string(),
                dirname: "https://nodejs.org/download/release/v15.14.0".to_string(),
                extname: ".tar.gz".to_string(),
            },
        ),
        (
            "16.0.0",
            "https://nodejs.org/download/release/",
            "darwin",
            "arm64",
            NodeArtifactAddress {
                basename: "node-v16.0.0-darwin-arm64".to_string(),
                dirname: "https://nodejs.org/download/release/v16.0.0".to_string(),
                extname: ".tar.gz".to_string(),
            },
        ),
    ];
    for (version, base_url, platform, arch, expected) in cases {
        let actual = get_node_artifact_address(GetNodeArtifactAddressOptions {
            version,
            base_url,
            platform,
            arch,
            libc: None,
        });
        assert_eq!(&actual, expected);
    }
}

/// Mirrors upstream's
/// `getNodeArtifactAddress with libc=musl appends -musl suffix to arch`
/// test.
#[test]
fn libc_musl_appends_suffix_to_arch() {
    let actual = get_node_artifact_address(GetNodeArtifactAddressOptions {
        version: "22.0.0",
        base_url: "https://unofficial-builds.nodejs.org/download/release/",
        platform: "linux",
        arch: "x64",
        libc: Some("musl"),
    });
    assert_eq!(
        actual,
        NodeArtifactAddress {
            basename: "node-v22.0.0-linux-x64-musl".to_string(),
            dirname: "https://unofficial-builds.nodejs.org/download/release/v22.0.0".to_string(),
            extname: ".tar.gz".to_string(),
        },
    );
}
