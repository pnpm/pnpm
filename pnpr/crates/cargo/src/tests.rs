use super::{
    CrateArchiveError, CrateDocument, CrateNameError, DependencyKind, IndexConfig,
    PublishBodyError, PublishMetadata, crate_filename, download_url, parse_index,
    parse_publish_body, render_index, sparse_index_path, validate_crate_archive,
    validate_crate_archive_with_limit, validate_crate_name,
};
use serde_json::json;
use std::io::Write as _;

pub(crate) fn crate_archive(root: &str, files: &[(&str, &str)]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for (path, contents) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, format!("{root}/{path}"), contents.as_bytes()).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

pub(crate) fn publish_body(metadata: &serde_json::Value, archive: &[u8]) -> Vec<u8> {
    let metadata = serde_json::to_vec(metadata).unwrap();
    let mut body = Vec::new();
    body.write_all(&(metadata.len() as u32).to_le_bytes()).unwrap();
    body.write_all(&metadata).unwrap();
    body.write_all(&(archive.len() as u32).to_le_bytes()).unwrap();
    body.write_all(archive).unwrap();
    body
}

#[test]
fn crate_names_follow_crates_io_rules() {
    for valid in ["a", "serde", "serde_json", "Inflector", "_private", "a-b_c9"] {
        validate_crate_name(valid).unwrap_or_else(|err| panic!("{valid}: {err}"));
    }
    assert_eq!(validate_crate_name(""), Err(CrateNameError::Empty));
    assert!(matches!(validate_crate_name("9lives"), Err(CrateNameError::InvalidStart { .. })));
    assert!(matches!(validate_crate_name("-dash"), Err(CrateNameError::InvalidStart { .. })));
    assert!(matches!(validate_crate_name("a.b"), Err(CrateNameError::InvalidCharacter { .. })));
    assert!(matches!(validate_crate_name("a/b"), Err(CrateNameError::InvalidCharacter { .. })));
    assert!(matches!(validate_crate_name("é"), Err(CrateNameError::InvalidStart { .. })));
    let long = "a".repeat(65);
    assert!(matches!(validate_crate_name(&long), Err(CrateNameError::TooLong { .. })));
}

#[test]
fn sparse_index_paths_follow_the_cargo_layout() {
    assert_eq!(sparse_index_path("a"), "1/a");
    assert_eq!(sparse_index_path("ab"), "2/ab");
    assert_eq!(sparse_index_path("abc"), "3/a/abc");
    assert_eq!(sparse_index_path("serde"), "se/rd/serde");
    assert_eq!(sparse_index_path("Inflector"), "in/fl/inflector");
}

#[test]
fn download_url_expands_every_template_marker() {
    assert_eq!(
        download_url("https://static.crates.io/crates", "serde", "1.0.0", "abc"),
        "https://static.crates.io/crates/serde/1.0.0/download",
    );
    assert_eq!(
        download_url(
            "https://dl.test/{prefix}/{lowerprefix}/{crate}-{version}-{sha256-checksum}",
            "Inflector",
            "0.11.4",
            "abc"
        ),
        "https://dl.test/In/fl/in/fl/Inflector-0.11.4-abc",
    );
    assert_eq!(
        download_url("https://dl.test/{crate}/{version}", "ab", "1.0.0", "x"),
        "https://dl.test/ab/1.0.0",
    );
}

#[test]
fn index_config_for_a_registry_points_back_at_it() {
    let config = IndexConfig::for_registry("http://pnpr.test/~crates", true);
    assert_eq!(
        serde_json::to_value(&config).unwrap(),
        json!({
            "dl": "http://pnpr.test/~crates/api/v1/crates",
            "api": "http://pnpr.test/~crates",
            "auth-required": true,
        }),
    );
    let public = IndexConfig::for_registry("http://pnpr.test/~crates", false);
    assert!(serde_json::to_value(&public).unwrap().get("auth-required").is_none());
    let parsed = IndexConfig::parse(
        br#"{"dl":"https://static.crates.io/crates","api":"https://crates.io"}"#,
    )
    .unwrap();
    assert_eq!(parsed.dl, "https://static.crates.io/crates");
    assert!(!parsed.auth_required);
}

#[test]
fn publish_body_splits_into_metadata_and_archive() {
    let archive = crate_archive("demo-0.1.0", &[("Cargo.toml", "[package]\nname = \"demo\"")]);
    let metadata = json!({ "name": "demo", "vers": "0.1.0" });
    let body = publish_body(&metadata, &archive);
    let (parsed, bytes) = parse_publish_body(&body).unwrap();
    assert_eq!(parsed.name, "demo");
    assert_eq!(parsed.vers, "0.1.0");
    assert_eq!(bytes, archive.as_slice());
}

#[test]
fn publish_body_rejects_truncation_and_trailing_bytes() {
    assert!(matches!(
        parse_publish_body(&[1, 0]),
        Err(PublishBodyError::Truncated { expected: 2 })
    ));
    let overrun = [10, 0, 0, 0, b'{', b'}'];
    assert!(matches!(
        parse_publish_body(&overrun),
        Err(PublishBodyError::LengthOverrun { field: "metadata", declared: 10, remaining: 2 }),
    ));
    let mut body = publish_body(&json!({ "name": "demo", "vers": "0.1.0" }), b"crate");
    body.push(0);
    assert!(matches!(
        parse_publish_body(&body),
        Err(PublishBodyError::TrailingBytes { trailing: 1 })
    ));
    let not_json = publish_body(&json!("string"), b"");
    assert!(matches!(parse_publish_body(&not_json), Err(PublishBodyError::Metadata(_))));
}

#[test]
fn publish_metadata_becomes_an_index_entry() {
    let metadata: PublishMetadata = serde_json::from_value(json!({
        "name": "demo",
        "vers": "0.1.0",
        "deps": [
            { "name": "serde", "version_req": "^1", "features": ["derive"], "optional": false, "default_features": true, "target": null, "kind": "normal", "registry": null, "explicit_name_in_toml": null },
            { "name": "tokio", "version_req": "^1.40", "optional": true, "kind": "dev", "explicit_name_in_toml": "rt" },
        ],
        "features": { "default": ["std"], "std": [], "rt": ["dep:tokio"], "weak": ["serde?/derive"] },
        "authors": ["someone"],
        "description": "A demo",
        "links": "demo-sys",
        "rust_version": "1.85",
        "badges": {},
    }))
    .unwrap();
    metadata.validate().unwrap();
    let entry = metadata.into_index_entry("00ff".to_string());
    assert_eq!(
        serde_json::to_value(&entry).unwrap(),
        json!({
            "name": "demo",
            "vers": "0.1.0",
            "deps": [
                { "name": "serde", "req": "^1", "features": ["derive"], "optional": false, "default_features": true, "kind": "normal" },
                { "name": "rt", "req": "^1.40", "features": [], "optional": true, "default_features": true, "kind": "dev", "package": "tokio" },
            ],
            "cksum": "00ff",
            "features": { "default": ["std"], "std": [] },
            "yanked": false,
            "links": "demo-sys",
            "v": 2,
            "features2": { "rt": ["dep:tokio"], "weak": ["serde?/derive"] },
            "rust_version": "1.85",
        }),
    );
    assert_eq!(entry.deps[1].kind, DependencyKind::Dev);
}

#[test]
fn publish_metadata_without_new_feature_syntax_stays_schema_one() {
    let metadata: PublishMetadata = serde_json::from_value(
        json!({ "name": "demo", "vers": "0.1.0", "features": { "a": ["b"] } }),
    )
    .unwrap();
    let entry = metadata.into_index_entry("00".to_string());
    assert_eq!(entry.v, 1);
    assert_eq!(entry.features2, None);
    assert!(serde_json::to_value(&entry).unwrap().get("features2").is_none());
}

#[test]
fn publish_metadata_validation_rejects_bad_names_and_versions() {
    let bad_version: PublishMetadata =
        serde_json::from_value(json!({ "name": "demo", "vers": "not-semver" })).unwrap();
    assert!(bad_version.validate().is_err());
    let bad_name: PublishMetadata =
        serde_json::from_value(json!({ "name": "../demo", "vers": "1.0.0" })).unwrap();
    assert!(bad_name.validate().is_err());
    let bad_dep: PublishMetadata = serde_json::from_value(json!({
        "name": "demo",
        "vers": "1.0.0",
        "deps": [{ "name": "x/y", "version_req": "^1" }],
    }))
    .unwrap();
    assert!(bad_dep.validate().is_err());
    let bad_req: PublishMetadata = serde_json::from_value(json!({
        "name": "demo",
        "vers": "1.0.0",
        "deps": [{ "name": "x", "version_req": "not a req" }],
    }))
    .unwrap();
    assert!(bad_req.validate().is_err());
}

#[test]
fn index_files_round_trip_through_the_document() {
    let text = concat!(
        r#"{"name":"demo","vers":"0.1.0","deps":[],"cksum":"aa","features":{},"yanked":false,"v":1}"#,
        "\n",
        r#"{"name":"demo","vers":"0.2.0","deps":[],"cksum":"bb","features":{},"yanked":true,"v":2,"features2":{"x":["dep:y"]},"unknown_future_field":1}"#,
        "\n",
    );
    let entries = parse_index(text).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries[1].yanked);
    let mut document = CrateDocument::new("demo");
    document.versions.clone_from(&entries);
    let reparsed = CrateDocument::parse(&document.to_bytes()).unwrap();
    assert_eq!(reparsed, document);
    assert_eq!(reparsed.version("0.2.0").unwrap().cksum, "bb");
    let rendered = render_index(&reparsed.versions);
    assert_eq!(rendered.lines().count(), 2);
    assert_eq!(parse_index(&rendered).unwrap(), entries);
}

#[test]
fn index_parse_reports_the_offending_line() {
    let err =
        parse_index("{\"name\":\"a\",\"vers\":\"1\",\"cksum\":\"x\"}\n\nnot json\n").unwrap_err();
    assert_eq!(err.line, 3);
}

#[test]
fn crate_archive_must_hold_the_crate_it_claims() {
    let good = crate_archive(
        "demo-0.1.0",
        &[("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\""), ("src/lib.rs", "")],
    );
    validate_crate_archive(&good, "demo", "0.1.0").unwrap();

    let renamed = validate_crate_archive(&good, "demo", "0.2.0").unwrap_err();
    assert!(matches!(renamed, CrateArchiveError::EntryOutsideRoot { .. }), "{renamed}");

    let no_manifest = crate_archive("demo-0.1.0", &[("src/lib.rs", "")]);
    assert!(matches!(
        validate_crate_archive(&no_manifest, "demo", "0.1.0"),
        Err(CrateArchiveError::MissingManifest { .. }),
    ));

    assert!(matches!(
        validate_crate_archive(b"not gzip at all", "demo", "0.1.0"),
        Err(CrateArchiveError::Read(_)),
    ));

    assert_eq!(crate_filename("demo", "0.1.0"), "demo-0.1.0.crate");
}

#[test]
fn crate_archive_limit_allows_equality_and_rejects_overflow() {
    let archive = crate_archive(
        "demo-0.1.0",
        &[("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"")],
    );
    let mut decoder = flate2::read::GzDecoder::new(archive.as_slice());
    let size = std::io::copy(&mut decoder, &mut std::io::sink()).unwrap();
    validate_crate_archive_with_limit(&archive, "demo", "0.1.0", size).unwrap();
    assert!(matches!(
        validate_crate_archive_with_limit(&archive, "demo", "0.1.0", size - 1),
        Err(CrateArchiveError::TooLarge)
    ));
}

#[test]
fn crate_archive_rejects_traversal_and_links() {
    for (path, entry_type) in [
        ("demo-0.1.0/../../outside", tar::EntryType::Regular),
        (r"demo-0.1.0/..\outside", tar::EntryType::Regular),
        ("demo-0.1.0/Cargo.toml", tar::EntryType::Symlink),
        ("demo-0.1.0/Cargo.toml", tar::EntryType::Link),
    ] {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.as_mut_bytes()[..path.len()].copy_from_slice(path.as_bytes());
        header.set_size(0);
        header.set_mode(0o644);
        header.set_entry_type(entry_type);
        if entry_type != tar::EntryType::Regular {
            header.set_link_name("../../outside").unwrap();
        }
        header.set_cksum();
        builder.append(&header, std::io::empty()).unwrap();
        let archive = builder.into_inner().unwrap().finish().unwrap();
        assert!(
            matches!(
                validate_crate_archive(&archive, "demo", "0.1.0"),
                Err(CrateArchiveError::EntryOutsideRoot { .. }
                    | CrateArchiveError::UnsupportedEntry { .. })
            ),
            "{path}",
        );
    }
}

#[test]
fn crate_archive_manifest_must_match_publish_metadata() {
    for manifest in [
        "",
        "[invalid",
        "[package]",
        "[package]\nname = 'other'\nversion = '0.1.0'",
        "[package]\nname = 'demo'\nversion = '0.2.0'",
    ] {
        let archive = crate_archive("demo-0.1.0", &[("Cargo.toml", manifest)]);
        assert!(
            matches!(
                validate_crate_archive(&archive, "demo", "0.1.0"),
                Err(CrateArchiveError::InvalidManifest { .. })
            ),
            "{manifest}",
        );
    }
}

#[test]
fn crate_archive_limit_counts_concatenated_gzip_members() {
    let mut archive = crate_archive(
        "demo-0.1.0",
        &[("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"")],
    );
    let size =
        std::io::copy(&mut flate2::read::GzDecoder::new(archive.as_slice()), &mut std::io::sink())
            .unwrap();
    let mut second = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    second.write_all(&[0; 1024]).unwrap();
    archive.extend(second.finish().unwrap());
    assert!(matches!(
        validate_crate_archive_with_limit(&archive, "demo", "0.1.0", size),
        Err(CrateArchiveError::TooLarge)
    ));
    validate_crate_archive_with_limit(&archive, "demo", "0.1.0", size + 1024).unwrap();
}

#[test]
fn crate_archive_accepts_an_explicit_root_directory() {
    let archive = crate_archive(
        "demo-0.1.0",
        &[("Cargo.toml", "[package]\nname = \"demo\"\nversion = \"0.1.0\"")],
    );
    for entry_type in [tar::EntryType::Directory, tar::EntryType::Regular] {
        let mut root = tar::Header::new_gnu();
        root.set_path("demo-0.1.0").unwrap();
        root.set_entry_type(entry_type);
        root.set_size(0);
        root.set_mode(0o755);
        root.set_cksum();
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(root.as_bytes()).unwrap();
        std::io::copy(&mut flate2::read::GzDecoder::new(archive.as_slice()), &mut encoder).unwrap();
        let with_root = encoder.finish().unwrap();
        assert_eq!(
            validate_crate_archive(&with_root, "demo", "0.1.0").is_ok(),
            entry_type.is_dir(),
        );
    }
}
