use super::split_variant_snapshots;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pretty_assertions::assert_eq;

fn entry(dependencies: &[(&str, &str)], optional: &[(&str, &str)]) -> SnapshotEntry {
    let map = |pairs: &[(&str, &str)]| {
        (!pairs.is_empty()).then(|| {
            pairs
                .iter()
                .map(|(name, version)| (name.parse().unwrap(), version.parse().unwrap()))
                .collect()
        })
    };
    SnapshotEntry {
        dependencies: map(dependencies),
        optional_dependencies: map(optional),
        ..SnapshotEntry::default()
    }
}

#[test]
fn a_variant_with_its_own_dependencies_gets_its_own_snapshot() {
    let manifest = serde_json::json!({
        "dependencies": { "semver": "^7.0.0" },
        "dist": { "pnpr": { "variants": [
            {
                "platforms": [{ "os": "linux", "cpu": "x64", "libc": "glibc" }],
                "manifest": { "dependencies": { "semver": "^7.0.0", "libvips": "1.0.0" } },
            },
            { "platforms": [{ "os": "darwin", "cpu": "arm64" }], "manifest": {} },
        ] } },
    });
    let key: PackageKey = "sharp@0.35.0(react@18.3.1)".parse().unwrap();
    let full = entry(&[("semver", "7.7.0"), ("libvips", "1.0.0"), ("react", "18.3.1")], &[]);

    let (base, variants) = split_variant_snapshots(&key, Some(&manifest), full);

    assert_eq!(base, entry(&[("semver", "7.7.0"), ("react", "18.3.1")], &[]));
    assert_eq!(variants.len(), 1, "a variant whose snapshot equals the version's has none");
    assert_eq!(variants[0].0.to_string(), "sharp@0.35.0/variants/linux-x64(react@18.3.1)");
    assert_eq!(
        variants[0].1,
        entry(&[("semver", "7.7.0"), ("libvips", "1.0.0"), ("react", "18.3.1")], &[]),
    );
}
