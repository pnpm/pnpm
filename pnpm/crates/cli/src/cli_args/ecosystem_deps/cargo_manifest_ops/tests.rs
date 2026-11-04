use super::format_cargo_saved_spec;
use semver::Version;

#[test]
fn format_cargo_saved_spec_preserves_exact_requirements() {
    let saved = format_cargo_saved_spec("=1.3.1", &Version::parse("1.3.6").unwrap(), None);

    assert_eq!(saved, "=1.3.6");
}

#[test]
fn format_cargo_saved_spec_prefers_an_explicit_range_over_an_exact_requirement() {
    let saved =
        format_cargo_saved_spec("=1.3.1", &Version::parse("2.0.0").unwrap(), Some("^2.0.0"));

    assert_eq!(saved, "^2.0.0");
}

#[test]
fn format_cargo_saved_spec_omits_build_metadata_from_an_explicit_range() {
    let saved = format_cargo_saved_spec(
        "1.1.8",
        &Version::parse("1.1.9+spec-1.1.1").unwrap(),
        Some("^1.1.9+spec-1.1.1"),
    );

    assert_eq!(saved, "^1.1.9");
}

#[test]
fn format_cargo_saved_spec_omits_build_metadata() {
    let saved = format_cargo_saved_spec(
        "1.1.8+spec-1.1.0",
        &Version::parse("1.1.9+spec-1.1.1").unwrap(),
        None,
    );

    assert_eq!(saved, "1.1.9");
}

#[test]
fn format_cargo_saved_spec_keeps_the_existing_operator() {
    let resolved = Version::parse("1.4.0").unwrap();

    assert_eq!(format_cargo_saved_spec("~1.3.1", &resolved, None), "~1.4.0");
    assert_eq!(format_cargo_saved_spec("1.3.1", &resolved, None), "1.4.0");
    assert_eq!(format_cargo_saved_spec("1.3.0+exp", &resolved, None), "1.4.0");
    assert_eq!(format_cargo_saved_spec(">=1.3, <2", &resolved, None), ">=1.4.0, <2");
}

#[test]
fn format_cargo_saved_spec_keeps_a_requirement_without_a_lower_bound() {
    let resolved = Version::parse("2.0.0").unwrap();

    assert_eq!(format_cargo_saved_spec("<=2.0.0", &resolved, None), "<=2.0.0");
    assert_eq!(format_cargo_saved_spec("<=2.0.0+spec", &resolved, None), "<=2.0.0");
    assert_eq!(format_cargo_saved_spec("2.*", &resolved, None), "2.*");
    assert_eq!(format_cargo_saved_spec("<2", &resolved, None), "^2.0.0");
}

#[test]
fn format_cargo_saved_spec_writes_a_bare_explicit_requirement_as_given() {
    let resolved = Version::parse("2.0.0").unwrap();

    assert_eq!(format_cargo_saved_spec("~1.0.0", &resolved, Some("2.0.0")), "2.0.0");
    assert_eq!(format_cargo_saved_spec("~1.0.0", &resolved, Some("2")), "2");
}
