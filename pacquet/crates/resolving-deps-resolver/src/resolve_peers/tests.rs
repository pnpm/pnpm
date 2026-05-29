use super::satisfies_with_prereleases;

#[test]
fn satisfies_handles_basic_ranges() {
    assert!(satisfies_with_prereleases("1.2.3", "^1.0.0"));
    assert!(!satisfies_with_prereleases("2.0.0", "^1.0.0"));
    assert!(satisfies_with_prereleases("18.0.0", "*"));
}

#[test]
fn satisfies_falls_back_to_equality_for_unparsable_ranges() {
    assert!(satisfies_with_prereleases("workspace:^1.0.0", "workspace:^1.0.0"));
    assert!(!satisfies_with_prereleases("1.0.0", "workspace:^1.0.0"));
}

#[test]
fn satisfies_accepts_prerelease_against_non_prerelease_range() {
    // Mirrors Yarn's `satisfiesWithPrereleases` carve-out: a peer
    // candidate at `18.0.0-rc.1` should satisfy a `^18.0.0` peer
    // requirement. node-semver's default `satisfies` rejects this
    // pairing, so the prerelease-strip retry has to catch it.
    assert!(satisfies_with_prereleases("18.0.0-rc.1", "^18.0.0"));
    assert!(satisfies_with_prereleases("1.2.3-beta.0", "^1.2.0"));
    // Out-of-range prereleases still fail.
    assert!(!satisfies_with_prereleases("19.0.0-rc.1", "^18.0.0"));
}
