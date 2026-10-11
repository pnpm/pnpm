use super::build_tools_identity;
use crate::{Lockfile, PackageKey, Prefix};
use pretty_assertions::assert_eq;
use text_block_macros::text_block;

const LOCKFILE: &str = text_block! {
    "lockfileVersion: '9.0'"
    ""
    "settings:"
    "  autoInstallPeers: true"
    "  excludeLinksFromLockfile: false"
    "  lockBuildTools: true"
    ""
    "importers:"
    ""
    "  .:"
    "    dependencies:"
    "      example:"
    "        specifier: github:org/example#5c8f9a"
    "        version: git+https://github.com/org/example.git#5c8f9a"
    ""
    "packages:"
    ""
    "  example@git+https://github.com/org/example.git#5c8f9a:"
    "    resolution: {commit: 5c8f9a, repo: https://github.com/org/example.git, type: git}"
    "    version: 1.0.0"
    ""
    "  yarn@tool:1.22.22:"
    "    resolution: {integrity: sha512-mQ==, tarball: https://registry.npmjs.org/yarn/-/yarn-1.22.22.tgz}"
    ""
    "snapshots:"
    ""
    "  example@git+https://github.com/org/example.git#5c8f9a:"
    "    buildDependencies:"
    "      yarn: tool:1.22.22"
    ""
};

fn parse(text: &str) -> Lockfile {
    serde_saphyr::from_str(text).expect("parse lockfile")
}

fn example_key() -> PackageKey {
    "example@git+https://github.com/org/example.git#5c8f9a".parse().expect("parse key")
}

#[test]
fn build_dependencies_round_trip() {
    let lockfile = parse(LOCKFILE);
    let saved = lockfile.to_yaml_string().expect("serialize lockfile");
    assert_eq!(saved, LOCKFILE);
}

#[test]
fn build_dependency_keys_carry_the_tool_prefix() {
    let lockfile = parse(LOCKFILE);
    let snapshot = &lockfile.snapshots.as_ref().expect("snapshots")[&example_key()];
    let keys: Vec<_> = snapshot.build_dependency_keys().collect();
    dbg!(&keys);
    assert_eq!(keys.len(), 1);
    let (alias, key) = &keys[0];
    assert_eq!(alias.to_string(), "yarn");
    assert_eq!(key.to_string(), "yarn@tool:1.22.22");
    assert_eq!(key.suffix.prefix(), Prefix::Tool);
}

#[test]
fn build_tools_identity_follows_the_recorded_bytes() {
    let lockfile = parse(LOCKFILE);
    let tampered = parse(&LOCKFILE.replace("sha512-mQ==", "sha512-mg=="));
    let identity = |lockfile: &Lockfile| {
        let snapshot = &lockfile.snapshots.as_ref().expect("snapshots")[&example_key()];
        build_tools_identity(snapshot, lockfile.packages.as_ref())
    };

    let recorded = identity(&lockfile);
    assert!(recorded.is_some());
    assert_eq!(identity(&lockfile), recorded);
    assert_ne!(identity(&tampered), recorded);
    assert_eq!(
        build_tools_identity(&crate::SnapshotEntry::default(), lockfile.packages.as_ref()),
        None,
    );
}

/// What a frozen install will run as a build tool must be the tool's own
/// artifact: anything else in the lockfile is refused.
#[test]
fn a_build_tool_entry_must_be_the_tools_own_artifact() {
    let artifact = |key: &str, resolution: &str| {
        let resolution: crate::LockfileResolution =
            serde_saphyr::from_str(resolution).expect("parse resolution");
        let key: PackageKey = key.parse().expect("parse key");
        super::build_tool_artifact(&key, &resolution)
            .map(|artifact| match artifact {
                super::BuildToolArtifact::Registry { package, .. } => package,
                super::BuildToolArtifact::Archive => "archive",
            })
    };
    let berry = "{integrity: sha512-mQ==, tarball: https://registry.npmjs.org/@yarnpkg/cli-dist/-/cli-dist-4.9.2.tgz}";
    let variations = "{type: variations, variants: []}";

    assert_eq!(artifact("yarn@tool:4.9.2", berry), Some("@yarnpkg/cli-dist"));
    assert_eq!(artifact("yarn@tool:1.22.22", berry), Some("yarn"));
    assert_eq!(artifact("yarn@tool:6.0.0", variations), Some("archive"));
    assert_eq!(artifact("node@tool:24.6.0", variations), Some("archive"));
    // A local file, a tarball without integrity, and a registry resolution
    // whose URL would come from the tool's name.
    assert_eq!(
        artifact("yarn@tool:1.22.22", "{tarball: 'file:./yarn.tgz', integrity: sha512-mQ==}"),
        None,
    );
    assert_eq!(
        artifact(
            "yarn@tool:1.22.22",
            "{tarball: https://registry.npmjs.org/yarn/-/yarn-1.22.22.tgz}"
        ),
        None,
    );
    assert_eq!(artifact("yarn@tool:4.9.2", "{integrity: sha512-mQ==}"), None);
    // A tool in the wrong shape, an unknown tool, and a key without `tool:`.
    assert_eq!(artifact("node@tool:24.6.0", berry), None);
    assert_eq!(artifact("yarn@tool:6.0.0", berry), None);
    assert_eq!(artifact("evil@tool:1.0.0", berry), None);
    assert_eq!(artifact("yarn@1.22.22", berry), None);
}
