use super::{DeclaredSpecifiers, calc_specifier_for_workspace_dep};
use pnpm_config::SaveWorkspaceProtocol;
use pnpm_registry::RangeSpecStyle;
use pretty_assertions::assert_eq;

/// `calc_specifier_for_workspace_dep` for a non-aliased dependency on
/// `my-lib@1.2.3`, with no previous lockfile entry.
fn fresh(bare: &str, protocol: SaveWorkspaceProtocol) -> String {
    calc_specifier_for_workspace_dep(
        DeclaredSpecifiers { prev: None, bare: Some(bare) },
        Some("my-lib"),
        "my-lib",
        Some("1.2.3"),
        protocol,
        RangeSpecStyle::Major,
    )
}

#[test]
fn rolling_collapses_a_pinned_range_to_the_bare_operator() {
    use SaveWorkspaceProtocol::Rolling;
    assert_eq!(fresh("workspace:^1.2.3", Rolling), "workspace:^");
    assert_eq!(fresh("workspace:~1.2.3", Rolling), "workspace:~");
    assert_eq!(fresh("workspace:1.2.3", Rolling), "workspace:*");
}

/// An already-rolling specifier is returned untouched rather than
/// re-derived, so `workspace:*` never silently becomes `workspace:^`.
#[test]
fn rolling_preserves_an_operator_only_specifier() {
    use SaveWorkspaceProtocol::Rolling;
    for specifier in ["workspace:*", "workspace:^", "workspace:~"] {
        assert_eq!(fresh(specifier, Rolling), specifier);
    }
}

/// A bare-semver request (no `workspace:` prefix) still rolls, because
/// the dependency did resolve to a workspace package.
#[test]
fn rolling_applies_to_a_bare_semver_request() {
    use SaveWorkspaceProtocol::Rolling;
    assert_eq!(fresh("^1.0.0", Rolling), "workspace:^");
    assert_eq!(fresh("~1.0.0", Rolling), "workspace:~");
    assert_eq!(fresh("1.0.0", Rolling), "workspace:*");
    assert_eq!(fresh("=1.0.0", Rolling), "workspace:*");
}

/// A tag or an unrecoverable range has no operator to carry over, so it
/// falls back to `^`.
#[test]
fn rolling_falls_back_to_caret() {
    use SaveWorkspaceProtocol::Rolling;
    assert_eq!(fresh("latest", Rolling), "workspace:^");
    assert_eq!(
        calc_specifier_for_workspace_dep(
            DeclaredSpecifiers { prev: None, bare: None },
            Some("my-lib"),
            "my-lib",
            Some("1.2.3"),
            Rolling,
            RangeSpecStyle::Major,
        ),
        "workspace:^",
    );
}

/// The previous manifest entry wins over what the user typed, so
/// re-running `add` on a dependency already saved as `workspace:~`
/// keeps the tilde.
#[test]
fn rolling_prefers_the_previous_specifier() {
    assert_eq!(
        calc_specifier_for_workspace_dep(
            DeclaredSpecifiers { prev: Some("workspace:~"), bare: Some("^1.0.0") },
            Some("my-lib"),
            "my-lib",
            Some("1.2.3"),
            SaveWorkspaceProtocol::Rolling,
            RangeSpecStyle::Major,
        ),
        "workspace:~",
    );
}

#[test]
fn pinned_writes_the_resolved_version_with_the_default_operator() {
    use SaveWorkspaceProtocol::On;
    assert_eq!(fresh("^1.0.0", On), "workspace:^1.2.3");
    assert_eq!(
        calc_specifier_for_workspace_dep(
            DeclaredSpecifiers { prev: None, bare: Some("^1.0.0") },
            Some("my-lib"),
            "my-lib",
            Some("1.2.3"),
            On,
            RangeSpecStyle::Patch,
        ),
        "workspace:1.2.3",
    );
}

/// Unlike the rolling form, the pinned form reads the operator off the
/// *previous* specifier only — matching pnpm, which does not consult
/// the freshly typed one here.
#[test]
fn pinned_takes_its_operator_from_the_previous_specifier() {
    assert_eq!(
        calc_specifier_for_workspace_dep(
            DeclaredSpecifiers { prev: Some("workspace:~1.0.0"), bare: Some("^1.0.0") },
            Some("my-lib"),
            "my-lib",
            Some("1.2.3"),
            SaveWorkspaceProtocol::On,
            RangeSpecStyle::Major,
        ),
        "workspace:~1.2.3",
    );
}

/// A `^`/`~` range over a prerelease would not match the prerelease it
/// was resolved from, so it is written exactly.
#[test]
fn pinned_writes_a_prerelease_exactly() {
    assert_eq!(
        calc_specifier_for_workspace_dep(
            DeclaredSpecifiers { prev: None, bare: Some("^1.0.0") },
            Some("my-lib"),
            "my-lib",
            Some("2.0.0-beta.1"),
            SaveWorkspaceProtocol::On,
            RangeSpecStyle::Major,
        ),
        "workspace:2.0.0-beta.1",
    );
}

/// `Off` still renders a `workspace:` specifier — declining to use one
/// at all is the caller's decision, not this function's.
#[test]
fn off_renders_the_pinned_shape() {
    use SaveWorkspaceProtocol::Off;
    assert_eq!(fresh("^1.0.0", Off), "workspace:^1.2.3");
    assert_eq!(fresh("workspace:^1.0.0", Off), "workspace:^1.2.3");
}

/// An aliased dependency names its target inside the protocol, so the
/// entry keeps pointing at the workspace package rather than at
/// whatever shares the install name.
#[test]
fn an_alias_names_its_target_inside_the_protocol() {
    let specifier = |protocol| {
        calc_specifier_for_workspace_dep(
            DeclaredSpecifiers { prev: None, bare: Some("workspace:^1.0.0") },
            Some("lib-alias"),
            "my-lib",
            Some("1.2.3"),
            protocol,
            RangeSpecStyle::Major,
        )
    };
    assert_eq!(specifier(SaveWorkspaceProtocol::Rolling), "workspace:my-lib@^");
    assert_eq!(specifier(SaveWorkspaceProtocol::On), "workspace:my-lib@^1.2.3");
}

/// Without a resolved version there is nothing to pin to, so the pinned
/// form falls back to the rolling shape rather than inventing one.
#[test]
fn a_missing_version_falls_back_to_the_rolling_shape() {
    let specifier = |protocol| {
        calc_specifier_for_workspace_dep(
            DeclaredSpecifiers { prev: None, bare: Some("workspace:^") },
            Some("my-lib"),
            "my-lib",
            None,
            protocol,
            RangeSpecStyle::Major,
        )
    };
    assert_eq!(specifier(SaveWorkspaceProtocol::Rolling), "workspace:^");
    assert_eq!(specifier(SaveWorkspaceProtocol::On), "workspace:^");
}
