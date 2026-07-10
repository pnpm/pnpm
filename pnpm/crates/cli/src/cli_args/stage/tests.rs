use flate2::{Compression, write::GzEncoder};
use pretty_assertions::assert_eq;
use serde_json::json;

use super::{
    StageError, is_uuid, parse_package_filter, render_stage_item, render_stage_publish_summary,
    render_tarball_summary, require_stage_id,
    summarize_tarball::{create_tarball_filename, summarize_tarball},
};

const STAGE_ID: &str = "1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f";

fn params(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn is_uuid_accepts_hyphenated_uuids_only() {
    assert!(is_uuid(STAGE_ID));
    assert!(is_uuid("F8E7A45B-7A5F-4F31-8E6D-9DD1C6EF38C0"));
    assert!(!is_uuid("not-a-uuid"));
    assert!(!is_uuid("1de6f3db2ed94d72b3dd8f0e2b474a2f"));
    assert!(!is_uuid("1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2g"));
    assert!(!is_uuid(""));
}

#[test]
fn require_stage_id_validates_presence_and_shape() {
    let with_id = params(&["view", STAGE_ID]);
    let id = require_stage_id(&with_id, "view").expect("a valid stage id");
    assert_eq!(id, STAGE_ID);

    let missing = require_stage_id(&params(&["view"]), "view").expect_err("no id given");
    assert!(matches!(missing, StageError::StageIdRequired { subcommand: "view" }));

    let empty = require_stage_id(&params(&["view", ""]), "view").expect_err("an empty id");
    assert!(matches!(empty, StageError::StageIdRequired { .. }));

    let invalid = require_stage_id(&params(&["view", "abc"]), "view").expect_err("not a UUID");
    assert!(matches!(invalid, StageError::InvalidStageId));
}

#[test]
fn parse_package_filter_accepts_bare_names_and_star() {
    assert_eq!(parse_package_filter(None).expect("no filter"), None);
    assert_eq!(
        parse_package_filter(Some(&"@scope/example-package".to_owned())).expect("a scoped name"),
        Some("@scope/example-package".to_owned()),
    );
    assert_eq!(
        parse_package_filter(Some(&"pkg@*".to_owned())).expect("a star specifier"),
        Some("pkg".to_owned()),
    );
}

#[test]
fn parse_package_filter_rejects_version_specifiers() {
    let err = parse_package_filter(Some(&"pkg@1.0.0".to_owned()))
        .expect_err("a version specifier is not supported");
    assert!(matches!(err, StageError::VersionSpecifierUnsupported));
}

#[test]
fn render_stage_publish_summary_covers_the_three_outcomes() {
    let mut summary = sample_summary("pkg", "1.0.0");
    assert_eq!(render_stage_publish_summary(&summary, true), "+ pkg@1.0.0 (would stage)");
    assert_eq!(render_stage_publish_summary(&summary, false), "+ pkg@1.0.0 (staged)");
    summary.stage_id = Some(STAGE_ID.to_owned());
    assert_eq!(
        render_stage_publish_summary(&summary, false),
        format!("+ pkg@1.0.0 (staged with id {STAGE_ID})"),
    );
}

#[test]
fn render_stage_item_orders_known_fields_and_appends_the_rest() {
    let item = json!({
        "extra": {"nested": true},
        "id": STAGE_ID,
        "packageName": "@scope/example-package",
        "version": "1.2.3",
        "tag": "latest",
        "createdAt": "2026-03-16T09:00:00.000Z",
        "actor": "user",
        "actorType": "user",
        "shasum": "4f7f5f1d5bcf2f72f6e4d6c4f3b2812d8a2f6c19",
        "skipped": null,
    });
    let rendered = render_stage_item(&item);
    assert_eq!(
        rendered,
        format!(
            "id: {STAGE_ID}\npackage name: @scope/example-package\nversion: 1.2.3\ntag: \
             latest\ndate staged: 2026-03-16T09:00:00.000Z\nstaged by: user (user)\nshasum: \
             4f7f5f1d5bcf2f72f6e4d6c4f3b2812d8a2f6c19\nextra: {{\"nested\":true}}",
        ),
    );
}

#[test]
fn render_stage_item_renders_a_bare_actor_without_a_type() {
    let rendered = render_stage_item(&json!({ "actor": "user" }));
    assert_eq!(rendered, "staged by: user");
}

#[test]
fn create_tarball_filename_normalizes_scoped_names_and_appends_the_suffix() {
    assert_eq!(
        create_tarball_filename("@scope/pkg", "1.0.0", Some(STAGE_ID)).expect("a safe filename"),
        format!("scope-pkg-1.0.0-{STAGE_ID}.tgz"),
    );
    assert_eq!(
        create_tarball_filename("pkg", "1.0.0", None).expect("a safe filename"),
        "pkg-1.0.0.tgz",
    );
}

#[test]
fn create_tarball_filename_rejects_traversal_through_name_and_version() {
    let bad_name = create_tarball_filename("@scope/../../outside", "1.0.0", None)
        .expect_err("a traversal name");
    assert!(matches!(bad_name, StageError::InvalidPackageName { .. }));

    let bad_version = create_tarball_filename("@scope/pkg", "1.0.0/../../outside", None)
        .expect_err("a traversal version");
    assert!(matches!(bad_version, StageError::InvalidPackageVersion { .. }));
}

#[test]
fn summarize_tarball_reads_the_manifest_files_and_digests() {
    let tarball = gzipped_tarball(&[
        ("package/package.json", r#"{"name":"@scope/pkg","version":"1.0.0"}"#),
        ("package/lib/index.js", "module.exports = 1"),
        ("package/README.md", "hi"),
    ]);

    let summary = summarize_tarball(&tarball).expect("a well-formed tarball");

    assert_eq!(summary.name, "@scope/pkg");
    assert_eq!(summary.version, "1.0.0");
    assert_eq!(summary.id, "@scope/pkg@1.0.0");
    assert_eq!(summary.filename, "scope-pkg-1.0.0.tgz");
    assert_eq!(summary.entry_count, 3);
    assert_eq!(summary.size, tarball.len() as u64);
    let paths: Vec<&str> = summary.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["lib/index.js", "package.json", "README.md"]);
    assert!(summary.integrity.starts_with("sha512-"), "integrity: {}", summary.integrity);
    assert_eq!(summary.shasum.len(), 40);
    assert!(summary.bundled.is_empty());
}

#[test]
fn summarize_tarball_accepts_a_plain_uncompressed_tarball() {
    let tarball = plain_tarball(&[("package/package.json", r#"{"name":"pkg","version":"2.0.0"}"#)]);
    let summary = summarize_tarball(&tarball).expect("a plain tarball");
    assert_eq!(summary.name, "pkg");
    assert_eq!(summary.version, "2.0.0");
}

#[test]
fn summarize_tarball_collects_bundled_dependencies_from_node_modules() {
    let tarball = gzipped_tarball(&[
        ("package/package.json", r#"{"name":"pkg","version":"1.0.0"}"#),
        ("package/node_modules/dep/index.js", ""),
        ("package/node_modules/@scope/other/index.js", ""),
    ]);
    let summary = summarize_tarball(&tarball).expect("a tarball with bundled deps");
    assert_eq!(summary.bundled, ["@scope/other", "dep"]);
}

#[test]
fn summarize_tarball_requires_a_manifest_with_name_and_version() {
    let missing = summarize_tarball(&gzipped_tarball(&[("package/index.js", "")]))
        .expect_err("no manifest at all");
    assert_eq!(
        missing.code().map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_STAGE_TARBALL_MANIFEST_NOT_FOUND"),
    );

    let nameless =
        summarize_tarball(&gzipped_tarball(&[("package/package.json", r#"{"version":"1.0.0"}"#)]))
            .expect_err("a manifest without a name");
    assert_eq!(
        nameless.code().map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_STAGE_TARBALL_MANIFEST_NOT_FOUND"),
    );
}

#[test]
fn render_tarball_summary_matches_the_pnpm_layout() {
    let mut summary = sample_summary("pkg", "1.0.0");
    summary.files = vec![
        pacquet_publish::PublishSummaryFile { path: "index.js".to_owned() },
        pacquet_publish::PublishSummaryFile { path: "package.json".to_owned() },
    ];
    summary.entry_count = 2;
    let rendered = render_tarball_summary(&summary);
    assert_eq!(
        rendered,
        "package: pkg@1.0.0\nTarball Contents\nindex.js\npackage.json\nTarball Details\nname: \
         pkg\nversion: 1.0.0\nfilename: pkg-1.0.0.tgz\npackage size: 128\nunpacked size: \
         256\nshasum: abc\nintegrity: sha512-xyz\ntotal files: 2",
    );
}

fn sample_summary(name: &str, version: &str) -> pacquet_publish::PublishSummary {
    pacquet_publish::PublishSummary {
        id: format!("{name}@{version}"),
        name: name.to_owned(),
        version: version.to_owned(),
        size: 128,
        unpacked_size: 256,
        shasum: "abc".to_owned(),
        integrity: "sha512-xyz".to_owned(),
        filename: format!("{name}-{version}.tgz"),
        files: Vec::new(),
        entry_count: 0,
        bundled: Vec::new(),
        stage_id: None,
    }
}

fn plain_tarball(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, path, contents.as_bytes()).expect("append tar entry");
    }
    builder.into_inner().expect("finish the tar archive")
}

fn gzipped_tarball(entries: &[(&str, &str)]) -> Vec<u8> {
    let tar = plain_tarball(entries);
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    std::io::Write::write_all(&mut encoder, &tar).expect("gzip the tarball");
    encoder.finish().expect("finish the gzip stream")
}
