//! Integration-test boundary for ported tests whose subject under
//! test is not yet implemented. The module lives at the top of the
//! test binary so `just known-failures` (filter `^known_failures::`)
//! picks each test up.

mod known_failures {
    use pacquet_package_is_installable::{Engine, WantedEngine, check_engine};
    use pacquet_testing_utils::{
        allow_known_failure,
        known_failure::{KnownFailure, KnownResult},
    };

    const PACKAGE_ID: &str = "registry.npmjs.org/foo/1.0.0";

    fn current(node: &str, pnpm: Option<&str>) -> Engine {
        Engine { node: node.to_string(), pnpm: pnpm.map(str::to_string) }
    }

    fn wanted(node: Option<&str>, pnpm: Option<&str>) -> WantedEngine {
        WantedEngine { node: node.map(str::to_string), pnpm: pnpm.map(str::to_string) }
    }

    /// Boundary helper for the strict-upper-bound prerelease semantics
    /// pacquet's `satisfies_with_prerelease` doesn't fully implement.
    fn semver_strict_upper_bound_prerelease_handled() -> KnownResult<()> {
        Err(KnownFailure::new(
            "pacquet's strip-prerelease fallback approximates npm-semver's \
             `includePrerelease: true`, but over-accepts on a strict upper-bounded \
             range. Upstream `>=9.0.0` rejects `9.0.0-alpha.1` (alpha.1 < 9.0.0 in \
             semver order, no implicit `-0` floor when fully specified); pacquet's \
             strip turns the version into `9.0.0` which then satisfies `>=9.0.0`. \
             Fix path: either add the `nodejs-semver` fork (which exposes \
             `satisfies_with_prerelease(include_prerelease: bool)`) or open-code the \
             strict-upper-bound carve-out. Engine ranges of this shape are vanishingly \
             rare in real package.json files.",
        ))
    }

    /// Ports the third assertion of `pnpm is a prerelease version` at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/test/checkEngine.ts#L31-L35>.
    ///
    /// Lives under `known_failures` because pacquet's
    /// `satisfies_with_prerelease` accepts where upstream rejects. The
    /// `allow_known_failure!` call at the boundary keeps the test
    /// executable so the day pacquet implements byte-for-byte
    /// `includePrerelease: true` semantics, this test will start
    /// passing and lose its known-failure marker.
    #[test]
    fn pnpm_is_a_prerelease_version_strict_ge_full_version_does_not_satisfy() {
        allow_known_failure!(semver_strict_upper_bound_prerelease_handled());

        let err = check_engine(
            PACKAGE_ID,
            &wanted(None, Some(">=9.0.0")),
            &current("0.2.1", Some("9.0.0-alpha.1")),
        )
        .expect("valid node version")
        .expect("must report unsatisfied");
        assert_eq!(err.wanted.pnpm.as_deref(), Some(">=9.0.0"));
    }
}
