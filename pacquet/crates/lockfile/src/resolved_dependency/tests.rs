use super::{ImporterDepVersion, ParseImporterDepVersionError, ResolvedDependencySpec};
use crate::{PkgName, PkgNameVerPeer, PkgVerPeer};
use pretty_assertions::assert_eq;
use std::borrow::Cow;

#[test]
fn parses_regular_version() {
    let parsed: ImporterDepVersion = "4.0.0".parse().unwrap();
    assert_eq!(parsed.as_regular().map(ToString::to_string), Some("4.0.0".to_string()));
    assert!(parsed.as_link_target().is_none());
    let serialized: String = parsed.into();
    assert_eq!(serialized, "4.0.0");
}

#[test]
fn parses_regular_version_with_peer() {
    let parsed: ImporterDepVersion = "17.0.2(react@17.0.2)".parse().unwrap();
    assert!(matches!(parsed, ImporterDepVersion::Regular(_)));
}

#[test]
fn parses_link_version() {
    let parsed: ImporterDepVersion = "link:../shared".parse().unwrap();
    assert_eq!(parsed.as_link_target(), Some("../shared"));
    assert!(parsed.as_regular().is_none());
    let serialized: String = parsed.into();
    assert_eq!(serialized, "link:../shared");
}

#[test]
fn parses_link_with_absolute_path() {
    let parsed: ImporterDepVersion = "link:/abs/sibling".parse().unwrap();
    assert_eq!(parsed.as_link_target(), Some("/abs/sibling"));
}

#[test]
fn resolved_spec_deserialize_link() {
    let yaml = "specifier: workspace:*\nversion: link:../shared\n";
    let spec: ResolvedDependencySpec = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(spec.specifier, "workspace:*");
    assert_eq!(spec.version.as_link_target(), Some("../shared"));
}

#[test]
fn resolved_spec_deserialize_regular() {
    let yaml = "specifier: ^4.0.0\nversion: 4.0.0\n";
    let spec: ResolvedDependencySpec = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(spec.specifier, "^4.0.0");
    assert!(spec.version.as_regular().is_some());
}

/// Regression for the `version: '@zkochan/js-yaml@0.0.11'` shape
/// pnpm v11 writes for `catalog:` deps that resolve to a scoped alias.
#[test]
fn parses_scoped_alias_version() {
    let parsed: ImporterDepVersion = "@zkochan/js-yaml@0.0.11".parse().unwrap();
    let alias = parsed.as_alias().expect("alias variant");
    assert_eq!(alias.name.to_string(), "@zkochan/js-yaml");
    assert_eq!(alias.suffix.to_string(), "0.0.11");
    assert!(parsed.as_regular().is_none());
    assert!(parsed.as_link_target().is_none());
    let serialized: String = parsed.into();
    assert_eq!(serialized, "@zkochan/js-yaml@0.0.11");
}

#[test]
fn parses_unscoped_alias_version() {
    let parsed: ImporterDepVersion = "string-width@4.2.3".parse().unwrap();
    let alias = parsed.as_alias().expect("alias variant");
    assert_eq!(alias.name.to_string(), "string-width");
    assert_eq!(alias.suffix.to_string(), "4.2.3");
}

#[test]
fn parses_alias_version_with_peer() {
    let parsed: ImporterDepVersion = "react-dom@17.0.2(react@17.0.2)".parse().unwrap();
    let alias = parsed.as_alias().expect("alias variant");
    assert_eq!(alias.name.to_string(), "react-dom");
    assert_eq!(alias.suffix.to_string(), "17.0.2(react@17.0.2)");
    let serialized: String = parsed.into();
    assert_eq!(serialized, "react-dom@17.0.2(react@17.0.2)");
}

#[test]
fn resolved_spec_deserialize_alias() {
    let yaml = "specifier: 'catalog:'\nversion: '@zkochan/js-yaml@0.0.11'\n";
    let spec: ResolvedDependencySpec = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(spec.specifier, "catalog:");
    let alias = spec.version.as_alias().expect("alias variant");
    assert_eq!(alias.name.to_string(), "@zkochan/js-yaml");
    assert_eq!(alias.suffix.to_string(), "0.0.11");
}

#[test]
fn resolved_key_returns_alias_name_for_alias_variant() {
    let importer_key: PkgName = "js-yaml".parse().unwrap();

    let regular: ImporterDepVersion = "4.0.0".parse().unwrap();
    let regular_key = regular.resolved_key(&importer_key).expect("regular key");
    assert_eq!(regular_key.name.to_string(), "js-yaml");
    assert_eq!(regular_key.suffix.to_string(), "4.0.0");

    let alias: ImporterDepVersion = "@zkochan/js-yaml@0.0.11".parse().unwrap();
    let alias_key = alias.resolved_key(&importer_key).expect("alias key");
    assert_eq!(alias_key.name.to_string(), "@zkochan/js-yaml");
    assert_eq!(alias_key.suffix.to_string(), "0.0.11");

    let link: ImporterDepVersion = "link:../shared".parse().unwrap();
    assert!(link.resolved_key(&importer_key).is_none());
}

#[test]
fn as_alias_returns_none_for_non_alias_variants() {
    let regular: ImporterDepVersion = "4.0.0".parse().unwrap();
    assert!(regular.as_alias().is_none());

    let link: ImporterDepVersion = "link:../shared".parse().unwrap();
    assert!(link.as_alias().is_none());
}

#[test]
fn ver_peer_returns_snapshot_version_for_each_variant() {
    let regular: ImporterDepVersion = "17.0.2(react@17.0.2)".parse().unwrap();
    let regular_ver = regular.ver_peer().expect("regular ver_peer");
    assert_eq!(regular_ver.to_string(), "17.0.2(react@17.0.2)");

    let alias: ImporterDepVersion = "react-dom@17.0.2(react@17.0.2)".parse().unwrap();
    let alias_ver = alias.ver_peer().expect("alias ver_peer");
    assert_eq!(alias_ver.to_string(), "17.0.2(react@17.0.2)");

    let link: ImporterDepVersion = "link:../shared".parse().unwrap();
    assert!(link.ver_peer().is_none());
}

/// The reproduction case from pnpm/pnpm#11776 is a `https://codeload...`
/// tarball URL.
#[test]
fn parses_non_semver_url_version() {
    let url = "https://codeload.github.com/whiskeysockets/libsignal-node/tar.gz/0848bc83347720c322c5087f3bd0d6cd086ffa4b";
    let parsed: ImporterDepVersion = url.parse().unwrap();
    let regular = parsed.as_regular().expect("regular variant");
    assert_eq!(regular.to_string(), url);
    let serialized: String = parsed.into();
    assert_eq!(serialized, url);
}

#[test]
fn parse_errors_on_mismatched_parens() {
    let err = "1.21.3(".parse::<ImporterDepVersion>().unwrap_err();
    match err {
        ParseImporterDepVersionError::Parse { value, .. } => {
            assert_eq!(value, "1.21.3(");
        }
        other => panic!("expected Parse variant, got {other:?}"),
    }
}

#[test]
fn parse_errors_on_invalid_alias_shape() {
    let err = "@scope/no-at-sign".parse::<ImporterDepVersion>().unwrap_err();
    match err {
        ParseImporterDepVersionError::ParseAlias { value, .. } => {
            assert_eq!(value, "@scope/no-at-sign");
        }
        other => panic!("expected ParseAlias variant, got {other:?}"),
    }
}

#[test]
fn try_from_cow_parses_all_three_shapes() {
    let regular = ImporterDepVersion::try_from(Cow::Borrowed("4.0.0")).unwrap();
    assert!(regular.as_regular().is_some());

    let alias = ImporterDepVersion::try_from(Cow::Owned("react-dom@17.0.2".to_string())).unwrap();
    assert!(alias.as_alias().is_some());

    let link = ImporterDepVersion::try_from(Cow::Borrowed("link:../shared")).unwrap();
    assert_eq!(link.as_link_target(), Some("../shared"));
}

#[test]
fn serialize_alias_writes_name_at_version_string() {
    let alias: ImporterDepVersion = "@zkochan/js-yaml@0.0.11".parse().unwrap();
    let yaml = serde_saphyr::to_string(&alias).unwrap();
    assert!(yaml.contains("@zkochan/js-yaml@0.0.11"), "got: {yaml}");
}

#[test]
fn serialize_link_writes_link_prefix() {
    let link = ImporterDepVersion::Link("../shared".to_string());
    let yaml = serde_saphyr::to_string(&link).unwrap();
    assert!(yaml.contains("link:../shared"), "got: {yaml}");
}

#[test]
fn from_typed_versions_into_importer_dep_version() {
    let ver: PkgVerPeer = "4.0.0".parse().unwrap();
    let regular: ImporterDepVersion = ver.into();
    assert!(matches!(regular, ImporterDepVersion::Regular(_)));

    let alias_inner: PkgNameVerPeer = "react-dom@17.0.2".parse().unwrap();
    let alias: ImporterDepVersion = alias_inner.into();
    assert!(matches!(alias, ImporterDepVersion::Alias(_)));
}

/// Pinning both guards against a divergence that would surface as a
/// lockfile formatting bug.
#[test]
fn display_matches_string_conversion() {
    let cases = [
        "4.0.0",
        "17.0.2(react@17.0.2)",
        "@zkochan/js-yaml@0.0.11",
        "react-dom@17.0.2(react@17.0.2)",
        "link:../shared",
    ];
    for case in cases {
        let parsed: ImporterDepVersion = case.parse().unwrap();
        let via_display = format!("{parsed}");
        let via_into: String = parsed.into();
        assert_eq!(via_display, via_into, "Display vs From<_> for {case}");
        assert_eq!(via_display, case);
    }
}
