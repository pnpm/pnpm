use node_semver::Version;

use super::{
    is_any_version_range,
    is_valid_semver_range,
    parse_strict_semver_range,
};

#[test]
fn empty_and_blank_ranges_are_any_version() {
    assert!(is_any_version_range(""));
    assert!(is_any_version_range(" "));
    assert!(is_any_version_range("   "));
    assert!(is_any_version_range("\t"));
}

#[test]
fn a_union_with_an_empty_member_is_any_version() {
    assert!(is_any_version_range("||"));
    assert!(is_any_version_range("^1.0.0 || "));
    assert!(is_any_version_range("|| ^1.0.0"));
    assert!(is_any_version_range("^1.0.0||"));
}

#[test]
fn an_empty_member_widens_a_union_whose_siblings_do_not_parse() {
    assert!(is_any_version_range("latest || "));
    assert!(is_any_version_range("|| latest"));
    assert!(is_any_version_range("garbage ||"));
}

#[test]
fn bounded_ranges_are_not_any_version() {
    assert!(!is_any_version_range("^1.0.0"));
    assert!(!is_any_version_range("1.0.0"));
    assert!(!is_any_version_range(">=1.0.0 <2.0.0"));
    assert!(!is_any_version_range("^1.0.0 || ^2.0.0"));
    assert!(!is_any_version_range("latest"));
}

#[test]
fn any_version_spellings_are_valid_ranges() {
    assert!(is_valid_semver_range(""));
    assert!(is_valid_semver_range(" "));
    assert!(is_valid_semver_range("||"));
    assert!(is_valid_semver_range("^1.0.0 || "));
}

#[test]
fn ordinary_ranges_stay_valid_and_tags_stay_invalid() {
    for valid in [
        "*",
        "x",
        "X",
        "1.x",
        "1.2.x",
        "*.*",
        "^1.0.0",
        "^ 1.0.0",
        "~1.0.0",
        "~> 1.0.0",
        "~ > 1.0.0",
        "=1.0.0",
        "= 1.0.0",
        "=v1.0.0",
        "v1.0.0",
        "==*",
        "==1.x",
        "v=1.x",
        "= v1.x",
        ">=1.0.0 <2.0.0",
        ">= 1.0.0 <= 2.0.0",
        "> =1.0.0",
        "< =2.0.0",
        "1.0.0 - 2.0.0",
        "1.0 - 2.0",
        "v1.0.0 - v2.0.0",
        "v 1.x - 2.0.0",
        "1.0.0 - v 1.x",
        "1.0.0 - v 2.0.0-alpha",
        "^1.0.0 || ^2.0.0",
        "1.2.3-alpha.1+build.2",
        "1.2+build",
    ] {
        assert!(is_valid_semver_range(valid), "expected valid: {valid:?}");
    }
    assert!(!is_valid_semver_range("latest"));
    assert!(!is_valid_semver_range("npm:bar@^5"));
}

#[test]
fn a_union_with_an_unparsable_member_is_not_a_valid_range() {
    assert!(!is_valid_semver_range("latest || "));
    assert!(!is_valid_semver_range("|| latest"));
    assert!(!is_valid_semver_range("garbage ||"));
    assert!(!is_valid_semver_range("npm:bar@^5 || "));
}

#[test]
fn a_comparator_set_with_an_unparsable_token_is_not_a_valid_range() {
    for invalid in [
        "bar@>=7.0.0 <8.0.0",
        "@scope/pkg@>=1.0.0 <2.0.0",
        "bar@6.0.0 - 8.0.0",
        "npm:bar@>=7.0.0 <8.0.0",
        "npm:bar@6.0.0 - 8.0.0",
        "foo >=1.0.0",
        ">=1.0.0 foo",
        "1.0.0 - 2.0.0 <3.0.0",
        "0.1.0 1.0.0 - 2.0.0",
        " - 2.0.0",
        "1.0.0 -",
        "01.02.03",
        "1.0.0alpha",
        "v 1.0.0",
        "> = 1.0.0",
        "< = 2.0.0",
        "> = *",
        "< = *",
        "> = 1.x",
        "< = 1.2",
        "== *",
        "== 1.x",
        "==1.0.0",
        "v= 1.x",
        "v=1.0.0",
        "=v 1.x",
        "1.0.0 - v 2.0.0",
        "=1.0.0 - =2.0.0",
        "=**",
        "1.x.0",
        "x.1.0",
        "1.2-alpha",
        "1.0.0-01",
        "^900719925474099.0.0",
        "9007199254740991.0.0",
        ">=1.0.0 <9007199254740991.0.0",
    ] {
        assert!(!is_valid_semver_range(invalid), "expected invalid: {invalid:?}");
        assert!(parse_strict_semver_range(invalid).is_none(), "expected None: {invalid:?}");
    }
}

#[test]
fn parse_strict_semver_range_handles_any_and_wildcard_operators() {
    let v0 = Version::parse("0.5.0").unwrap();
    let v1 = Version::parse("1.2.3").unwrap();
    let v2 = Version::parse("2.0.0").unwrap();
    for unbounded in [
        "",
        " ",
        "^1.0.0 || ",
        "^x",
        "~x",
        "~*",
        "=*",
        "~>x",
        "> =*",
        "< =*",
        "> =x",
        "< =x",
        ">=0.0.0",
    ] {
        let parsed = parse_strict_semver_range(unbounded)
            .unwrap_or_else(|| panic!("expected Some for {unbounded:?}"));
        assert!(parsed.satisfies(&v0), "for {unbounded:?}");
        assert!(parsed.satisfies(&v1), "for {unbounded:?}");
        assert!(parsed.satisfies(&v2), "for {unbounded:?}");
    }
    let tilde_wildcard = parse_strict_semver_range("~1.x.0").unwrap();
    assert!(tilde_wildcard.satisfies(&v1));
    assert!(!tilde_wildcard.satisfies(&v2));
}

#[test]
fn parse_strict_semver_range_preserves_every_comparator_bound() {
    let v05 = Version::parse("0.5.0").unwrap();
    let v10 = Version::parse("1.0.0").unwrap();
    let v125 = Version::parse("1.2.5").unwrap();
    let v130 = Version::parse("1.3.0").unwrap();
    let v20 = Version::parse("2.0.0").unwrap();
    let v30 = Version::parse("3.0.0").unwrap();

    let gte = parse_strict_semver_range("> =1.0.0").unwrap();
    assert!(!gte.satisfies(&v05));
    assert!(gte.satisfies(&v10));
    assert!(gte.satisfies(&v20));

    let lte = parse_strict_semver_range("< =2.0.0").unwrap();
    assert!(lte.satisfies(&v10));
    assert!(lte.satisfies(&v20));
    assert!(!lte.satisfies(&v30));

    for caret_one in ["^1.x.0", ">=0.5.0 ^1.x.0", "^=1.0.0", "^ = 1.0.0"] {
        let parsed = parse_strict_semver_range(caret_one).unwrap();
        assert!(!parsed.satisfies(&v05), "for {caret_one:?}");
        assert!(parsed.satisfies(&v125), "for {caret_one:?}");
        assert!(!parsed.satisfies(&v20), "for {caret_one:?}");
    }

    for minor_one_two in ["1.2+build", ">=1.0.0 1.2+build", "~ =1.2.0", "~ = 1.2.0"] {
        let parsed = parse_strict_semver_range(minor_one_two).unwrap();
        assert!(!parsed.satisfies(&v10), "for {minor_one_two:?}");
        assert!(parsed.satisfies(&v125), "for {minor_one_two:?}");
        assert!(!parsed.satisfies(&v130), "for {minor_one_two:?}");
    }

    for js_ws in [">=1.0.0\n<2.0.0", ">=1.0.0\u{00a0}<2.0.0"] {
        let parsed = parse_strict_semver_range(js_ws).unwrap();
        assert!(!parsed.satisfies(&v05));
        assert!(parsed.satisfies(&v125));
        assert!(!parsed.satisfies(&v20));
    }

    for hyphen_star in ["1.0.0 - *", "1.0.0 - =*", "1.0.0 - =x"] {
        let parsed = parse_strict_semver_range(hyphen_star).unwrap();
        assert!(!parsed.satisfies(&v05), "for {hyphen_star:?}");
        assert!(parsed.satisfies(&v10), "for {hyphen_star:?}");
        assert!(parsed.satisfies(&v30), "for {hyphen_star:?}");
    }

    let star_upper = parse_strict_semver_range("* - 2.0.0").unwrap();
    assert!(star_upper.satisfies(&v05));
    assert!(star_upper.satisfies(&v20));
    assert!(!star_upper.satisfies(&v30));
}

#[test]
fn parse_strict_semver_range_handles_empty_intersections_and_integer_overflow() {
    let v0 = Version::parse("0.0.0-0").unwrap();
    let v05 = Version::parse("0.5.0").unwrap();
    let v30 = Version::parse("3.0.0").unwrap();

    for empty in [">*", "<*", ">2.0.0 <1.0.0"] {
        let parsed = parse_strict_semver_range(empty).unwrap();
        assert!(!parsed.satisfies(&v0), "for {empty:?}");
        assert!(!parsed.satisfies(&v05), "for {empty:?}");
        assert!(!parsed.satisfies(&v30), "for {empty:?}");
    }

    let union_with_disjoint = parse_strict_semver_range(">2.0.0 <1.0.0 || ^3.0.0").unwrap();
    assert!(!union_with_disjoint.satisfies(&v05));
    assert!(union_with_disjoint.satisfies(&v30));

    assert!(is_valid_semver_range("900719925474099.0.0"));
    assert!(parse_strict_semver_range("900719925474099.0.0").is_some());
}
