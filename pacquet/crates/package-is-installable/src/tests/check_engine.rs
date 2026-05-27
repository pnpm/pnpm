//! Port of `config/package-is-installable/test/checkEngine.ts`
//! at <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/test/checkEngine.ts>.

use crate::{Engine, WantedEngine, check_engine};

const PACKAGE_ID: &str = "registry.npmjs.org/foo/1.0.0";

fn current(node: &str, pnpm: Option<&str>) -> Engine {
    Engine { node: node.to_string(), pnpm: pnpm.map(str::to_string) }
}

fn wanted(node: Option<&str>, pnpm: Option<&str>) -> WantedEngine {
    WantedEngine { node: node.map(str::to_string), pnpm: pnpm.map(str::to_string) }
}

#[test]
fn no_engine_defined() {
    assert!(
        check_engine(PACKAGE_ID, &wanted(None, None), &current("0.2.1", Some("1.1.2")))
            .expect("valid node version")
            .is_none(),
    );
}

#[test]
fn prerelease_node_version() {
    assert!(
        check_engine(
            PACKAGE_ID,
            &wanted(Some("^14.18.0 || >=16.0.0"), None),
            &current("21.0.0-nightly20230429c968361829", None),
        )
        .expect("valid node version")
        .is_none(),
    );
}

#[test]
fn node_version_too_old() {
    let err = check_engine(
        PACKAGE_ID,
        &wanted(Some("0.10.24"), None),
        &current("0.10.18", Some("1.1.2")),
    )
    .expect("valid node version")
    .expect("must report unsatisfied");
    assert_eq!(err.wanted.node.as_deref(), Some("0.10.24"));
}

#[test]
fn node_range_passed_in_instead_of_version() {
    // Upstream throws `ERR_PNPM_INVALID_NODE_VERSION` from inside
    // `checkEngine` when the current node version isn't an exact
    // semver. Pacquet returns the same condition as an `Err`.
    let result =
        check_engine(PACKAGE_ID, &wanted(Some("21.0.0"), None), &current(">=20.0.0", None));
    let err = result.expect_err("expected InvalidNodeVersionError");
    assert_eq!(err.node_version, ">=20.0.0");
}

#[test]
fn pnpm_version_too_old() {
    let err =
        check_engine(PACKAGE_ID, &wanted(None, Some("^1.4.6")), &current("0.2.1", Some("1.3.2")))
            .expect("valid node version")
            .expect("must report unsatisfied");
    assert_eq!(err.wanted.pnpm.as_deref(), Some("^1.4.6"));
}

#[test]
fn pnpm_is_a_prerelease_version_partial_major_only_satisfies() {
    // `pnpm: '9'` matches `9.0.0-alpha.1` under upstream
    // `includePrerelease: true`. Pacquet's strip-prerelease fallback
    // gets this correctly because a partial major range parses as
    // `>=9.0.0 <10.0.0-0`, and `9.0.0` satisfies.
    assert!(
        check_engine(
            PACKAGE_ID,
            &wanted(None, Some("9")),
            &current("0.2.1", Some("9.0.0-alpha.1"))
        )
        .expect("valid node version")
        .is_none(),
    );
}

#[test]
fn pnpm_is_a_prerelease_version_ge_major_satisfies() {
    assert!(
        check_engine(
            PACKAGE_ID,
            &wanted(None, Some(">=9")),
            &current("0.2.1", Some("9.0.0-alpha.1")),
        )
        .expect("valid node version")
        .is_none(),
    );
}

#[test]
fn engine_is_supported() {
    assert!(
        check_engine(
            PACKAGE_ID,
            &wanted(Some("10"), Some("1")),
            &current("10.2.1", Some("1.3.2")),
        )
        .expect("valid node version")
        .is_none(),
    );
}

// The strict-upper-bound prerelease semantics case lives in
// `tests/known_failures.rs` as an integration test so `just
// known-failures` (filter `^known_failures::`) picks it up. The
// helper expects the `known_failures` module to sit at the top of a
// test binary's path; a unit-test submodule would land at
// `tests::check_engine::known_failures::...` and fall outside that
// filter.
