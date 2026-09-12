use super::{
    DistributionKind, ProjectDocument, ProjectFile, UploadError, Yanked, escape_html,
    multipart::{FormPart, tests::encode_form},
    normalize_name, normalize_version, parse_distribution_filename, parse_upload,
    render_project_list_html, render_project_list_json, wants_json, wants_versioned_html,
};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn project_names_normalize_the_pep_503_way() {
    for raw in ["Demo_Pkg", "demo.pkg", "DEMO--PKG", "demo_-.pkg"] {
        assert_eq!(normalize_name(raw).unwrap(), "demo-pkg", "{raw}");
    }
    assert!(normalize_name("").is_err());
    assert!(normalize_name("-leading").is_err());
    assert!(normalize_name("has space").is_err());
    assert!(normalize_name("../etc").is_err());
}

#[test]
fn versions_normalize_the_pep_440_way() {
    assert_eq!(normalize_version("1.0").unwrap(), "1.0");
    assert_eq!(normalize_version("1.0.0rc1").unwrap(), "1.0.0rc1");
    assert_eq!(normalize_version("1.0.0-RC1").unwrap(), "1.0.0rc1");
    assert!(normalize_version("not a version").is_err());
}

#[test]
fn distribution_filenames_parse_to_project_and_version() {
    let wheel = parse_distribution_filename("Demo_Pkg-1.0.0-py3-none-any.whl").unwrap();
    assert_eq!(wheel.name, "demo-pkg");
    assert_eq!(wheel.version, "1.0.0");
    assert_eq!(wheel.kind, DistributionKind::Wheel);
    let build_tag =
        parse_distribution_filename("demo-1.0.0-1-cp312-cp312-linux_x86_64.whl").unwrap();
    assert_eq!(build_tag.version, "1.0.0");
    let sdist = parse_distribution_filename("demo_pkg-1.0.0.tar.gz").unwrap();
    assert_eq!(sdist.name, "demo-pkg");
    assert_eq!(sdist.kind, DistributionKind::Sdist);
    let zip = parse_distribution_filename("demo-pkg-2.0.zip").unwrap();
    assert_eq!((zip.name.as_str(), zip.version.as_str()), ("demo-pkg", "2.0"));

    for invalid in [
        "demo.whl",
        "demo-1.0.0.whl",
        "demo-1.0.0.exe",
        "../demo-1.0.0.tar.gz",
        "demo-notaversion.tar.gz",
        "a/b-1.0-py3-none-any.whl",
    ] {
        assert!(parse_distribution_filename(invalid).is_err(), "{invalid}");
    }
}

fn document() -> ProjectDocument {
    ProjectDocument {
        name: "demo-pkg".to_string(),
        files: vec![
            ProjectFile {
                filename: "demo_pkg-1.0.0.tar.gz".to_string(),
                url: None,
                hashes: BTreeMap::from([("sha256".to_string(), "aa".to_string())]),
                requires_python: Some(">=3.9".to_string()),
                yanked: Yanked::Flag(false),
                size: Some(12),
                upload_time: Some("2026-01-01T00:00:00Z".to_string()),
            },
            ProjectFile {
                filename: "demo_pkg-1.1.0-py3-none-any.whl".to_string(),
                url: Some("https://files.test/x/demo_pkg-1.1.0-py3-none-any.whl".to_string()),
                hashes: BTreeMap::from([("sha256".to_string(), "bb".to_string())]),
                requires_python: None,
                yanked: Yanked::Reason("broken <build>".to_string()),
                size: None,
                upload_time: None,
            },
        ],
    }
}

#[test]
fn project_document_renders_pep_691_json() {
    let json = document().render_json("http://pnpr.test/~pypi/files/demo-pkg");
    assert_eq!(
        json,
        json!({
            "meta": { "api-version": "1.1" },
            "name": "demo-pkg",
            "versions": ["1.0.0", "1.1.0"],
            "files": [
                {
                    "filename": "demo_pkg-1.0.0.tar.gz",
                    "url": "http://pnpr.test/~pypi/files/demo-pkg/demo_pkg-1.0.0.tar.gz",
                    "hashes": { "sha256": "aa" },
                    "requires-python": ">=3.9",
                    "yanked": false,
                    "size": 12,
                    "upload-time": "2026-01-01T00:00:00Z",
                },
                {
                    "filename": "demo_pkg-1.1.0-py3-none-any.whl",
                    "url": "http://pnpr.test/~pypi/files/demo-pkg/demo_pkg-1.1.0-py3-none-any.whl",
                    "hashes": { "sha256": "bb" },
                    "yanked": "broken <build>",
                },
            ],
        }),
    );
}

#[test]
fn project_document_renders_pep_503_html() {
    let html = document().render_html("http://pnpr.test/~pypi/files/demo-pkg/");
    assert!(html.contains("<title>Links for demo-pkg</title>"), "{html}");
    assert!(
        html.contains(concat!(
            r#"<a href="http://pnpr.test/~pypi/files/demo-pkg/demo_pkg-1.0.0.tar.gz#sha256=aa""#,
            r#" data-requires-python="&gt;=3.9">demo_pkg-1.0.0.tar.gz</a><br />"#,
        )),
        "{html}",
    );
    assert!(
        html.contains(concat!(
            r#"<a href="http://pnpr.test/~pypi/files/demo-pkg/demo_pkg-1.1.0-py3-none-any.whl#sha256=bb""#,
            r#" data-yanked="broken &lt;build&gt;">demo_pkg-1.1.0-py3-none-any.whl</a><br />"#,
        )),
        "{html}",
    );
}

#[test]
fn project_document_round_trips_and_reads_upstream_pages() {
    let document = document();
    let reparsed = ProjectDocument::parse(&document.to_bytes()).unwrap();
    assert_eq!(reparsed, document);
    assert_eq!(reparsed.file("demo_pkg-1.0.0.tar.gz").unwrap().sha256(), Some("aa"));
    assert!(reparsed.file("demo_pkg-1.1.0-py3-none-any.whl").unwrap().yanked.is_yanked());

    let upstream = json!({
        "meta": { "api-version": "1.1", "_last-serial": 1 },
        "name": "demo-pkg",
        "versions": ["1.0.0"],
        "files": [{
            "filename": "demo_pkg-1.0.0.tar.gz",
            "url": "../../packages/ab/cd/demo_pkg-1.0.0.tar.gz",
            "hashes": { "sha256": "aa" },
            "requires-python": ">=3.9",
            "yanked": false,
            "size": 12,
            "upload-time": "2026-01-01T00:00:00.000000Z",
            "core-metadata": { "sha256": "cc" },
            "data-dist-info-metadata": false,
        }],
    });
    let upstream = ProjectDocument::parse(&serde_json::to_vec(&upstream).unwrap()).unwrap();
    assert_eq!(
        upstream.files[0].url.as_deref(),
        Some("../../packages/ab/cd/demo_pkg-1.0.0.tar.gz"),
    );
    assert_eq!(upstream.files[0].sha256(), Some("aa"));
}

#[test]
fn project_lists_render_in_both_formats() {
    let json = render_project_list_json(["a", "b"]);
    assert_eq!(json["projects"], json!([{ "name": "a" }, { "name": "b" }]));
    let html = render_project_list_html("http://pnpr.test/~pypi/simple", ["a", "b<"]);
    assert!(html.contains(r#"<a href="http://pnpr.test/~pypi/simple/a/">a</a><br />"#), "{html}");
    assert!(
        html.contains(r#"<a href="http://pnpr.test/~pypi/simple/b&lt;/">b&lt;</a><br />"#),
        "{html}",
    );
}

#[test]
fn accept_negotiation_reads_the_pep_691_types() {
    assert!(wants_json(Some(
        "application/vnd.pypi.simple.v1+json, application/vnd.pypi.simple.v1+html;q=0.2"
    )));
    assert!(!wants_json(Some("text/html")));
    assert!(!wants_json(None));
    assert!(wants_versioned_html(Some("application/vnd.pypi.simple.v1+html")));
    assert!(!wants_versioned_html(Some("*/*")));
}

#[test]
fn html_escaping_covers_markup_characters() {
    assert_eq!(escape_html(r#"a&b<c>d"e'f"#), "a&amp;b&lt;c&gt;d&quot;e&#39;f");
}

fn upload_parts(action: &str) -> Vec<FormPart> {
    super::multipart::parse_form(
        "multipart/form-data; boundary=b",
        &encode_form(
            "b",
            &[
                (":action", None, action.as_bytes()),
                ("protocol_version", None, b"1"),
                ("name", None, b"Demo_Pkg"),
                ("version", None, b"1.0.0"),
                ("filetype", None, b"bdist_wheel"),
                ("sha256_digest", None, b"ABCD"),
                ("requires_python", None, b""),
                ("metadata_version", None, b"2.1"),
                ("content", Some("demo_pkg-1.0.0-py3-none-any.whl"), b"wheel bytes"),
            ],
        ),
    )
    .unwrap()
}

#[test]
fn upload_form_is_read_into_an_upload() {
    let upload = parse_upload(upload_parts("file_upload")).unwrap();
    assert_eq!(upload.name, "Demo_Pkg");
    assert_eq!(upload.version, "1.0.0");
    assert_eq!(upload.filetype, "bdist_wheel");
    assert_eq!(upload.filename, "demo_pkg-1.0.0-py3-none-any.whl");
    assert_eq!(upload.content, b"wheel bytes");
    assert_eq!(upload.sha256_digest.as_deref(), Some("abcd"));
    assert_eq!(upload.requires_python, None);
}

#[test]
fn upload_form_rejects_other_actions_and_missing_fields() {
    assert_eq!(parse_upload(upload_parts("submit")).unwrap_err(), UploadError::NotAFileUpload);
    let mut parts = upload_parts("file_upload");
    parts.retain(|part| part.name != "content");
    assert_eq!(parse_upload(parts).unwrap_err(), UploadError::MissingField("content"));
    let mut parts = upload_parts("file_upload");
    for part in &mut parts {
        if part.name == "content" {
            part.filename = None;
        }
    }
    assert_eq!(parse_upload(parts).unwrap_err(), UploadError::MissingFilename);
    let mut parts = upload_parts("file_upload");
    for part in &mut parts {
        if part.name == "protocol_version" {
            part.data = b"2".to_vec();
        }
    }
    assert_eq!(parse_upload(parts).unwrap_err(), UploadError::UnsupportedProtocolVersion);
}

#[test]
fn accept_negotiation_respects_quality_and_exact_media_types() {
    for accept in [
        "text/html, application/vnd.pypi.simple.v1+json;q=0",
        "text/html;q=0.9, application/vnd.pypi.simple.v1+json;q=0.1",
        "application/vnd.pypi.simple.v1+json;q=NaN",
        "application/vnd.pypi.simple.v1+json;q=2",
        "application/vnd.pypi.simple.v1+json-extra",
        "*/*",
        "text/*;q=0.9, application/vnd.pypi.simple.v1+json;q=0.1",
    ] {
        assert!(!wants_json(Some(accept)), "{accept}");
    }
    assert!(wants_json(Some("text/html;q=0.1, APPLICATION/VND.PYPI.SIMPLE.V1+JSON; Q=0.9")));
    assert!(!wants_versioned_html(Some("application/vnd.pypi.simple.v1+html;q=0")));
    assert!(!wants_versioned_html(Some(
        "application/vnd.pypi.simple.v1+html;q=0.1, text/html;q=0.9"
    )));
}

#[test]
fn file_urls_encode_filename_delimiters() {
    let mut document = document();
    document.files[0].filename = "demo_pkg-1.0.0+local-py3-none-a#b?c%20 d.whl".to_string();
    let expected =
        "https://pnpr.test/files/demo-pkg/demo_pkg-1.0.0%2Blocal-py3-none-a%23b%3Fc%2520%20d.whl";
    assert_eq!(
        document.render_json("https://pnpr.test/files/demo-pkg")["files"][0]["url"],
        expected,
    );
    assert!(
        document
            .render_html("https://pnpr.test/files/demo-pkg")
            .contains(&format!("{expected}#sha256=aa")),
    );
}
