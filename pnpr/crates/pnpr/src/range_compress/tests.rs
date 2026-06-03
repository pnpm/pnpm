use super::*;
use node_semver::Version;
use serde_json::json;

/// Assert a range compresses to `expected` and that the compressed
/// form denotes the exact same version set as the original under
/// node-semver (the same engine pnpm/pacquet resolve with).
fn assert_compresses(original: &str, expected: &str) {
    let got = compress_range(original);
    assert_eq!(got.as_deref(), Some(expected), "compress_range({original:?})");
    assert_equivalent(original, expected);
}

/// Assert a range is left untouched (no strictly-shorter equivalent).
fn assert_unchanged(original: &str) {
    assert_eq!(compress_range(original), None, "compress_range({original:?}) should be a no-op");
}

fn assert_equivalent(left: &str, right: &str) {
    assert_eq!(
        Range::parse(left).unwrap(),
        Range::parse(right).unwrap(),
        "{left:?} and {right:?} must denote the same version set",
    );
}

#[test]
fn caret_with_zero_minor_and_patch_becomes_bare_major() {
    assert_compresses("^1.0.0", "1");
    assert_compresses("^12.0.0", "12");
}

#[test]
fn caret_on_zero_major_becomes_major_minor() {
    // `^0.5.0` == `>=0.5.0 <0.6.0` == `0.5`.
    assert_compresses("^0.5.0", "0.5");
}

#[test]
fn tilde_with_zero_patch_becomes_major_minor() {
    assert_compresses("~1.2.0", "1.2");
    assert_compresses("~1.0.0", "1.0");
}

#[test]
fn x_ranges_collapse_to_partials() {
    assert_compresses("1.x", "1");
    assert_compresses("1.x.x", "1");
    assert_compresses("1.2.x", "1.2");
    assert_compresses("1.2.X", "1.2");
}

#[test]
fn hand_written_explicit_intervals_are_not_rewritten_to_caret() {
    // A caret/x-range's upper bound is `<2.0.0-0` (it excludes
    // prereleases of the next major), so it is *not* the same version
    // set as a hand-written `<2.0.0`. node-semver — and JS semver —
    // distinguish the two, so the conversion is rejected and the
    // canonical (whitespace-normalized) form is kept instead.
    assert_unchanged(">=1.0.0 <2.0.0");
    assert_compresses(">=1.2.3  <2.0.0", ">=1.2.3 <2.0.0");
}

#[test]
fn whitespace_is_normalized() {
    assert_compresses(">= 1.2.3 < 2.0.0", ">=1.2.3 <2.0.0");
}

#[test]
fn already_minimal_ranges_are_left_alone() {
    // Nonzero patch under a caret can't shrink to a partial.
    assert_unchanged("^1.2.3");
    // Caret-0 with a nonzero patch is a one-patch window; irreducible.
    assert_unchanged("^0.0.3");
    // Bare partials and wildcards are already shortest.
    assert_unchanged("1");
    assert_unchanged("1.2");
    assert_unchanged("*");
    // A pinned exact version is already as short as it gets.
    assert_unchanged("1.2.3");
}

#[test]
fn non_semver_specifiers_pass_through_untouched() {
    assert_unchanged("workspace:*");
    assert_unchanged("npm:other@^1.0.0");
    assert_unchanged("git+https://example.com/a/b.git");
    assert_unchanged("file:../local");
    assert_unchanged("latest");
}

#[test]
fn prerelease_ranges_are_not_rewritten() {
    // Prerelease handling is the one area where the JS/Rust engines
    // can diverge, so we conservatively leave these alone.
    assert_unchanged("^1.2.3-alpha.1 <2.0.0");
    assert_unchanged(">=1.2.3-rc.1 <2.0.0");
}

#[test]
fn compression_is_idempotent() {
    let once = compress_range("^1.0.0").unwrap();
    assert_eq!(once, "1");
    assert_eq!(compress_range(&once), None);
}

/// The compressed form must accept and reject exactly the versions the
/// original does at the interval boundaries.
#[test]
fn boundary_versions_match_after_compression() {
    let cases = [("^1.0.0", "1"), ("1.2.x", "1.2"), ("~1.2.0", "1.2")];
    let probes = ["0.9.9", "1.0.0", "1.2.2", "1.2.3", "1.2.4", "1.9.9", "2.0.0"];
    for (original, _expected) in cases {
        let before = Range::parse(original).unwrap();
        let compressed = compress_range(original).unwrap();
        let after = Range::parse(&compressed).unwrap();
        for probe in probes {
            let version = Version::parse(probe).unwrap();
            assert_eq!(
                before.satisfies(&version),
                after.satisfies(&version),
                "{original:?} vs {compressed:?} disagree on {probe}",
            );
        }
    }
}

#[test]
fn compress_version_dependencies_rewrites_every_map() {
    let mut version = json!({
        "name": "foo",
        "version": "1.0.0",
        "dependencies": { "bar": "^1.0.0", "qux": "git+https://x/y.git" },
        "peerDependencies": { "react": "^16.0.0" },
        "optionalDependencies": { "fsevents": "~2.3.0" },
        "peerDependenciesMeta": { "react": { "optional": true } },
    });

    compress_version_dependencies(&mut version);

    assert_eq!(version["dependencies"]["bar"], "1");
    // Non-semver specifier untouched.
    assert_eq!(version["dependencies"]["qux"], "git+https://x/y.git");
    assert_eq!(version["peerDependencies"]["react"], "16");
    assert_eq!(version["optionalDependencies"]["fsevents"], "2.3");
    // Non-range maps are left alone.
    assert_eq!(version["peerDependenciesMeta"]["react"]["optional"], true);
}

#[test]
fn compress_packument_dependencies_walks_all_versions() {
    let mut packument = json!({
        "name": "foo",
        "versions": {
            "1.0.0": { "dependencies": { "bar": "^1.0.0" } },
            "2.0.0": { "dependencies": { "bar": "2.x" } },
        }
    });

    compress_packument_dependencies(&mut packument);

    assert_eq!(packument["versions"]["1.0.0"]["dependencies"]["bar"], "1");
    assert_eq!(packument["versions"]["2.0.0"]["dependencies"]["bar"], "2");
}
