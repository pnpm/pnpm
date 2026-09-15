use super::{
    Lockfile, StalenessReason, manifest_from_json, satisfies_package_manifest, text_block,
};

/// Lockfile has no `importers["."]` entry — even though pacquet's
/// `Lockfile` type makes `importers` a map (so an empty map is a
/// valid shape), we still want to fail cleanly when the importer the
/// caller asked about isn't present.
#[test]
fn missing_importer_returns_no_importer() {
    let lockfile: Lockfile =
        serde_saphyr::from_str("lockfileVersion: '9.0'\n").expect("parse minimal lockfile");
    // We can't easily get a `ProjectSnapshot` out of an empty map,
    // so this test exercises the lookup-then-call shape on the
    // caller side: the caller uses `root_project()` which returns
    // `None`, and the `NoImporter` reason is constructed there.
    assert!(lockfile.root_project().is_none());
}

/// `NoImporter` renders with `importers["."]`-style formatting, not
/// the `{:?}` debug-format `importers."."`, which quotes short keys
/// like `.` poorly.
#[test]
fn no_importer_message_uses_bracket_quoted_id() {
    let reason = StalenessReason::NoImporter { importer_id: ".".to_string() };
    let rendered = reason.to_string();
    assert!(rendered.contains(r#"importers["."]"#), "expected bracket-quoted id, got {rendered:?}");
    assert!(
        !rendered.contains(r#"importers.".""#),
        "must not use Rust debug-format quoting, got {rendered:?}",
    );
}

#[test]
fn importer_empty_dev_dependencies_equivalent_to_absent() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    dependencies:"
        "      foo:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
        "    devDependencies: {}"
    })
    .expect("parse fixture lockfile");
    let importer = lockfile.root_project().expect("root importer present");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "dependencies": { "foo": "^1.0.0" }
    }"#,
    );
    assert!(satisfies_package_manifest(importer, &manifest, true, &|_: &str| false).is_ok());
}

/// Pins the group gate: removing the `matches!(... Prod | Optional)`
/// check inside `satisfies_package_manifest` makes this test fail,
/// because the filter would then incorrectly apply to dev entries.
#[test]
fn ignored_optional_dev_only_lockfile_entry_kept() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    devDependencies:"
        "      foo:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
    })
    .expect("parse lockfile");
    let importer = lockfile.root_project().expect("root importer");
    let (_dir, manifest) = manifest_from_json(
        r#"{
        "name": "x",
        "version": "1.0.0",
        "devDependencies": { "foo": "^1.0.0" }
    }"#,
    );
    let is_ignored: &dyn Fn(&str) -> bool = &|name: &str| name == "foo";
    assert!(satisfies_package_manifest(importer, &manifest, true, is_ignored).is_ok());
}
