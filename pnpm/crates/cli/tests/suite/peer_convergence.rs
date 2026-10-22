//! Peer fixtures installed through every path that binds peers, across the
//! settings that change how peers bind. For each fixture and settings row,
//! starting from a fresh resolution of the manifests:
//!
//! - a frozen install accepts the fresh lockfile and leaves it unchanged,
//! - `dedupe --check` passes,
//! - every project deploys, and its deployed lockfile installs frozen,
//! - a second resolution from scratch writes the same lockfile,
//! - adding the dependencies one install at a time writes the same lockfile,
//! - `dedupe` after those installs writes the same lockfile,
//! - applying the fixture's [`Move`]s over the fresh lockfile writes the
//!   lockfile a fresh resolution of the moved manifests writes, and moving
//!   back writes the fresh lockfile again.
//!
//! A [`Check`] that writes a lockfile may differ from the fresh one in the
//! entries a [`Divergence`] of the row owns.
//!
//! Every violation of a fixture is collected before the test fails, so one
//! run reports the whole matrix.
//!
//! With `PNPM_V11_BUNDLE` naming the pnpm v11 bundle (`pnpm11/pnpm/dist/pnpm.mjs`),
//! every fixture also runs through v11, apart from the rows that set the
//! v12-only `autoDedupe`, and each row's fresh v11 lockfile has to equal the
//! v12 one.

mod harness;

use harness::{Harness, Move, Project, project};

#[test]
fn satisfied_peer() {
    Harness::check_with_moves(
        vec![project(
            "app",
            &[("@pnpm.e2e/has-foo100-peer", "1.0.0"), ("@pnpm.e2e/foo", "100.0.0")],
        )],
        vec![Move { project: "app", alias: "@pnpm.e2e/foo", to: "100.1.0" }],
        vec![],
    );
}

#[test]
fn missing_peer() {
    Harness::check(vec![project("app", &[("@pnpm.e2e/has-foo100-peer", "1.0.0")])], vec![]);
}

#[test]
fn several_peers() {
    Harness::check_with_moves(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/abc", "1.0.0"),
                ("@pnpm.e2e/peer-a", "1.0.0"),
                ("@pnpm.e2e/peer-b", "1.0.0"),
                ("@pnpm.e2e/peer-c", "1.0.0"),
            ],
        )],
        vec![Move { project: "app", alias: "@pnpm.e2e/peer-c", to: "1.0.1" }],
        vec![],
    );
}

#[test]
fn optional_peers_partly_provided() {
    Harness::check_with_moves(
        vec![project(
            "app",
            &[("@pnpm.e2e/abc-optional-peers", "1.0.0"), ("@pnpm.e2e/peer-a", "1.0.0")],
        )],
        vec![Move { project: "app", alias: "@pnpm.e2e/peer-a", to: "1.0.1" }],
        vec![],
    );
}

#[test]
fn optional_peer_also_in_dependencies() {
    Harness::check_with_moves(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/has-optional-peer-also-in-deps", "1.0.0"),
                ("@pnpm.e2e/bravo-dep", "1.0.0"),
            ],
        )],
        vec![Move { project: "app", alias: "@pnpm.e2e/bravo-dep", to: "1.1.0" }],
        vec![],
    );
}

#[test]
fn optional_peer_also_in_dependencies_provided_by_another_project() {
    Harness::check_with_moves(
        vec![
            project("a", &[("@pnpm.e2e/has-optional-peer-also-in-deps", "1.0.0")]),
            project(
                "b",
                &[
                    ("@pnpm.e2e/has-optional-peer-also-in-deps", "1.0.0"),
                    ("@pnpm.e2e/bravo-dep", "1.0.0"),
                ],
            ),
        ],
        vec![Move { project: "b", alias: "@pnpm.e2e/bravo-dep", to: "1.1.0" }],
        vec![],
    );
}

#[test]
fn two_package_peer_cycle() {
    Harness::check(
        vec![project(
            "app",
            &[("@pnpm.e2e/circular-peer-a", "1.0.0"), ("@pnpm.e2e/circular-peer-b", "1.0.0")],
        )],
        vec![],
    );
}

#[test]
fn three_package_peer_cycle() {
    Harness::check_with_moves(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/circular-peers-1-of-3", "1.0.0"),
                ("@pnpm.e2e/circular-peers-2-of-3", "1.0.0"),
                ("@pnpm.e2e/circular-peers-3-of-3", "1.0.0"),
                ("@pnpm.e2e/peer-a", "1.0.0"),
                ("@pnpm.e2e/peer-b", "1.0.0"),
            ],
        )],
        vec![Move { project: "app", alias: "@pnpm.e2e/peer-a", to: "1.0.1" }],
        vec![],
    );
}

#[test]
fn complex_peer_cycle() {
    Harness::check(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/complex-circular-peers-a", "1.0.0"),
                ("@pnpm.e2e/complex-circular-peers-b", "1.0.0"),
                ("@pnpm.e2e/complex-circular-peers-c", "1.0.0"),
            ],
        )],
        vec![],
    );
}

#[test]
fn peer_cycle_with_a_shared_peer() {
    Harness::check(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/closure-lib-a", "1.0.0"),
                ("@pnpm.e2e/closure-lib-b", "1.0.0"),
                ("@pnpm.e2e/closure-peer-x", "1.0.0"),
            ],
        )],
        vec![],
    );
}

#[test]
fn peers_that_are_also_dependencies() {
    Harness::check(vec![project("app", &[("@pnpm.e2e/deadlock.a", "1.0.0")])], vec![]);
}

#[test]
fn peer_diamond() {
    Harness::check(
        vec![project(
            "app",
            &[("@pnpm.e2e/peer-diamond-bundle", "1.0.0"), ("@pnpm.e2e/peer-diamond-ts", "1.0.0")],
        )],
        vec![],
    );
}

#[test]
fn peer_of_itself() {
    Harness::check(vec![project("app", &[("@pnpm.e2e/peer-of-itself", "1.0.0")])], vec![]);
}

#[test]
fn peer_resolved_from_own_dependencies() {
    Harness::check(
        vec![project(
            "app",
            &[("@pnpm.e2e/pkg-with-resolved-peer", "1.0.0"), ("@pnpm.e2e/peer-b", "1.0.0")],
        )],
        vec![],
    );
}

#[test]
fn peer_of_a_peer() {
    Harness::check(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/has-has-y-peer-peer", "1.0.0"),
                ("@pnpm.e2e/has-y-peer", "1.0.0"),
                ("@pnpm/y", "1.0.0"),
            ],
        )],
        vec![],
    );
}

#[test]
fn projects_with_different_providers() {
    Harness::check_with_moves(
        vec![
            project("a", &[("@pnpm.e2e/has-foo100-peer", "1.0.0"), ("@pnpm.e2e/foo", "100.0.0")]),
            project("b", &[("@pnpm.e2e/has-foo100-peer", "1.0.0"), ("@pnpm.e2e/foo", "100.1.0")]),
        ],
        vec![Move { project: "a", alias: "@pnpm.e2e/foo", to: "100.1.0" }],
        vec![],
    );
}

#[test]
fn projects_with_conflicting_peer_ranges() {
    Harness::check(
        vec![
            project("a", &[("@pnpm.e2e/wants-peer-c-1", "1.0.0")]),
            project("b", &[("@pnpm.e2e/wants-peer-c-2", "1.0.0")]),
        ],
        vec![],
    );
}

#[test]
fn workspace_package_with_a_peer() {
    Harness::check_with_moves(
        vec![
            Project { dir: "lib", deps: &[], peers: &[("@pnpm.e2e/foo", "100")] },
            project("a", &[("lib", "workspace:*"), ("@pnpm.e2e/foo", "100.0.0")]),
            project("b", &[("lib", "workspace:*"), ("@pnpm.e2e/foo", "100.1.0")]),
        ],
        vec![Move { project: "b", alias: "@pnpm.e2e/foo", to: "100.0.0" }],
        vec![],
    );
}

#[test]
fn peer_provider_moves_out_of_its_range() {
    Harness::check_with_moves(
        vec![project(
            "app",
            &[("@pnpm.e2e/has-foo100-peer", "1.0.0"), ("@pnpm.e2e/foo", "100.0.0")],
        )],
        vec![Move { project: "app", alias: "@pnpm.e2e/foo", to: "2.0.0" }],
        vec![],
    );
}

/// The peer's provider is a dependency's own dependency, and another
/// dependency of it accepts any version of the peer.
#[test]
fn peer_provided_by_a_dependency_moves() {
    Harness::check_with_moves(
        vec![project(
            "app",
            &[
                ("@pnpm.e2e/has-foo100-peer", "1.0.0"),
                ("provider", "npm:@pnpm.e2e/has-foo-as-dep-and-subdep@1.0.0"),
            ],
        )],
        vec![Move {
            project: "app",
            alias: "provider",
            to: "npm:@pnpm.e2e/has-foo-100.1.0-dep-1@1.0.0",
        }],
        vec![],
    );
}

/// The only provider of the required peer is a workspace dependency's own
/// dependency, so an auto-installed peer follows it (pnpm/tasks#61).
#[test]
fn peer_provided_by_a_workspace_dependency_moves() {
    Harness::check_with_moves(
        vec![
            project("provider", &[("@pnpm.e2e/peer-c", "1.0.0")]),
            project("app", &[("@pnpm.e2e/wants-peer-c-1", "1.0.0"), ("provider", "workspace:*")]),
        ],
        vec![Move { project: "provider", alias: "@pnpm.e2e/peer-c", to: "1.0.1" }],
        vec![],
    );
}

/// Project `a` (walked before `b` in importer order) consumes `@pnpm.e2e/peer-c`
/// as a peer of `@pnpm.e2e/wants-peer-c-1@1.0.0`, while project `b` declares
/// `@pnpm.e2e/peer-c` directly and moves it from `1.0.0` to `1.0.1`
/// (pnpm/tasks#160).
#[test]
fn peer_provided_by_another_projects_direct_dependency_moves() {
    Harness::check_with_moves(
        vec![
            project("b", &[("@pnpm.e2e/peer-c", "1.0.0")]),
            project("a", &[("@pnpm.e2e/wants-peer-c-1", "1.0.0")]),
        ],
        vec![Move { project: "b", alias: "@pnpm.e2e/peer-c", to: "1.0.1" }],
        vec![],
    );
}
