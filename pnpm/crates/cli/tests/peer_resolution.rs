use pacquet_testing_utils::{
    allow_known_failure,
    known_failure::{KnownFailure, KnownResult},
};

fn compatible_peer_range_intersection() -> KnownResult<()> {
    Err(KnownFailure::new(
        "pacquet's auto-install peer range intersection currently handles only identical ranges; broader compatible semver intersections are not implemented",
    ))
}

fn locked_peer_context() -> KnownResult<()> {
    Err(KnownFailure::new(
        "pacquet does not yet carry lockedPeerContext and resolvedPeerProviderPaths through peer resolution",
    ))
}

#[test]
fn auto_installed_peer_uses_the_intersection_of_compatible_ranges() {
    allow_known_failure!(compatible_peer_range_intersection());
}

#[test]
fn compatible_locked_peer_provider_is_reused() {
    allow_known_failure!(locked_peer_context());
}

#[test]
fn locked_peer_provider_outside_the_current_range_is_not_reused() {
    allow_known_failure!(locked_peer_context());
}
