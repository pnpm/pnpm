use super::{
    copy_fixture, cyclonedx_component, pacquet, run_sbom_json, run_sbom_json_from_store,
    set_dependency_author, set_root_author, spdx_package,
};

#[test]
fn sbom_spdx_basic() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);

    assert_eq!(parsed["spdxVersion"], "SPDX-2.3");
    assert_eq!(parsed["dataLicense"], "CC0-1.0");

    let packages = parsed["packages"].as_array().expect("packages array");
    assert!(packages.len() > 1);

    let root = &packages[0];
    assert_eq!(root["name"], "simple-sbom-test");
    assert_eq!(root["versionInfo"], "1.0.0");
}

/// A blank root author reaches neither format, while a real dependency
/// author still does.
#[test]
fn sbom_omits_a_blank_root_author() {
    let tmp = copy_fixture("simple-sbom");
    set_root_author(tmp.path(), " \t\n");
    set_dependency_author(tmp.path(), "Dep Author");

    let cyclonedx = run_sbom_json_from_store(tmp.path(), "cyclonedx");
    assert!(
        cyclonedx["metadata"]["component"].get("authors").is_none(),
        "a whitespace-only root author must not be emitted",
    );
    assert_eq!(
        cyclonedx_component(&cyclonedx, "is-positive")["authors"],
        serde_json::json!([{ "name": "Dep Author" }]),
    );

    let spdx = run_sbom_json_from_store(tmp.path(), "spdx");
    assert!(
        spdx_package(&spdx, "simple-sbom-test").get("supplier").is_none(),
        "a whitespace-only root supplier must not be emitted",
    );
    assert_eq!(spdx_package(&spdx, "is-positive")["supplier"], "Person: Dep Author");
}

/// The mirror of [`sbom_omits_a_blank_root_author`]: the blank name sits on a
/// dependency, and in the object form of the `author` field.
#[test]
fn sbom_omits_a_blank_dependency_author() {
    let tmp = copy_fixture("simple-sbom");
    set_root_author(tmp.path(), "Root Author");
    set_dependency_author(tmp.path(), "   ");

    let cyclonedx = run_sbom_json_from_store(tmp.path(), "cyclonedx");
    assert_eq!(
        cyclonedx["metadata"]["component"]["authors"],
        serde_json::json!([{ "name": "Root Author" }]),
    );
    let component = cyclonedx_component(&cyclonedx, "is-positive");
    assert_eq!(component["description"], "sbom author fixture");
    assert!(
        component.get("authors").is_none(),
        "a whitespace-only dependency author must not be emitted",
    );

    let spdx = run_sbom_json_from_store(tmp.path(), "spdx");
    assert_eq!(spdx_package(&spdx, "simple-sbom-test")["supplier"], "Person: Root Author");
    let package = spdx_package(&spdx, "is-positive");
    assert_eq!(package["description"], "sbom author fixture");
    assert!(
        package.get("supplier").is_none(),
        "a whitespace-only dependency supplier must not be emitted",
    );
}

#[test]
fn sbom_spec_version_with_spdx_fails() {
    let tmp = copy_fixture("simple-sbom");
    let output = pacquet(
        tmp.path(),
        ["sbom", "--sbom-format", "spdx", "--lockfile-only", "--sbom-spec-version", "1.6"],
    )
    .output()
    .expect("run pacquet");
    assert!(!output.status.success());
}

#[test]
fn sbom_has_serial_number() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let serial = parsed["serialNumber"].as_str().expect("serialNumber");
    assert!(serial.starts_with("urn:uuid:"), "serialNumber should start with urn:uuid:");
}

#[test]
fn sbom_has_timestamp() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    assert!(parsed["metadata"]["timestamp"].is_string());
}

#[test]
fn sbom_has_tools() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let tools = parsed["metadata"]["tools"]["components"].as_array().expect("tools");
    assert!(tools.iter().any(|tool| tool["name"] == "pnpm"));
}

#[test]
fn sbom_root_license() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    let licenses = parsed["metadata"]["component"]["licenses"].as_array().expect("licenses");
    assert!(!licenses.is_empty());
}

#[test]
fn sbom_root_description() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &[]);
    assert!(parsed["metadata"]["component"]["description"].is_string());
}

#[test]
fn sbom_spdx_creation_info() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    assert!(parsed["creationInfo"]["created"].is_string());
    let creators = parsed["creationInfo"]["creators"].as_array().expect("creators");
    assert!(creators.iter().any(|creator| creator.as_str().unwrap().contains("pnpm")));
}

#[test]
fn sbom_spdx_creation_info_whole_seconds() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let created = parsed["creationInfo"]["created"].as_str().expect("created string");
    let shape: String = created
        .chars()
        .map(|character| if character.is_ascii_digit() { 'd' } else { character })
        .collect();
    assert_eq!(
        shape, "dddd-dd-ddTdd:dd:ddZ",
        "SPDX 2.3 (6.9) requires whole-second UTC timestamps (YYYY-MM-DDThh:mm:ssZ), got {created}",
    );
}

#[test]
fn sbom_spdx_describes_relationship() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let rels = parsed["relationships"].as_array().expect("relationships");
    assert!(rels.iter().any(|rel| rel["relationshipType"] == "DESCRIBES"));
}

#[test]
fn sbom_spdx_license_from_manifest() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let root = &parsed["packages"].as_array().expect("packages")[0];
    assert_eq!(root["licenseConcluded"], "ISC");
    assert_eq!(root["licenseDeclared"], "ISC");
}

#[test]
fn sbom_spdx_download_location() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let packages = parsed["packages"].as_array().expect("packages");
    let is_positive =
        packages.iter().find(|pkg| pkg["name"] == "is-positive").expect("is-positive");
    let dl = is_positive["downloadLocation"].as_str().expect("downloadLocation");
    assert!(dl.contains("registry.npmjs.org"), "should have registry URL, got {dl}");
}

#[test]
fn sbom_authors_in_metadata() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--sbom-authors", "Alice, Bob"]);
    let authors = parsed["metadata"]["authors"].as_array().expect("authors");
    assert_eq!(authors.len(), 2);
    assert_eq!(authors[0]["name"], "Alice");
    assert_eq!(authors[1]["name"], "Bob");
}

#[test]
fn sbom_supplier_in_metadata() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--sbom-supplier", "ACME Corp"]);
    assert_eq!(parsed["metadata"]["supplier"]["name"], "ACME Corp");
}

#[test]
fn sbom_schema_url_matches_spec_version() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "cyclonedx", &["--sbom-spec-version", "1.5"]);
    let schema = parsed["$schema"].as_str().expect("$schema");
    assert!(schema.contains("1.5"), "schema should match spec version 1.5, got {schema}");
}

#[test]
fn sbom_spdx_root_has_purpose() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let root = &parsed["packages"].as_array().expect("packages")[0];
    assert_eq!(root["primaryPackagePurpose"], "LIBRARY");
}

#[test]
fn sbom_spdx_application_type_purpose() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &["--sbom-type", "application"]);
    let root = &parsed["packages"].as_array().expect("packages")[0];
    assert_eq!(root["primaryPackagePurpose"], "APPLICATION");
}

#[test]
fn sbom_spdx_document_namespace_has_uuid() {
    let tmp = copy_fixture("simple-sbom");
    let parsed = run_sbom_json(tmp.path(), "spdx", &[]);
    let ns = parsed["documentNamespace"].as_str().expect("documentNamespace");
    assert!(ns.contains("spdx.org/spdxdocs/"), "namespace should contain spdx.org");
    let parts: Vec<&str> = ns.rsplitn(2, '-').collect();
    assert!(parts[0].len() >= 8, "namespace should end with UUID-like suffix");
}
