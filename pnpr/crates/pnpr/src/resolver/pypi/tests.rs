use super::{MAX_TOTAL_BYTES, index_url, metadata_from_wheel, within_budget};
use std::io::Write as _;

fn wheel_with(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in entries {
        archive
            .start_file::<_, ()>(*name, zip::write::SimpleFileOptions::default())
            .expect("start a wheel entry");
        archive.write_all(body.as_bytes()).expect("write a wheel entry");
    }
    archive.finish().expect("finish the wheel").into_inner()
}

#[test]
fn a_wheels_metadata_is_read_from_its_dist_info() {
    let wheel = wheel_with(&[
        ("demo/__init__.py", "value = 1\n"),
        ("demo-1.0.0.dist-info/RECORD", "demo/__init__.py,,\n"),
        ("demo-1.0.0.dist-info/METADATA", "Name: demo\nVersion: 1.0.0\n"),
    ]);

    let document = metadata_from_wheel(&wheel, "demo-1.0.0-py3-none-any.whl").expect("metadata");

    assert_eq!(
        String::from_utf8(document).expect("metadata is text"),
        "Name: demo\nVersion: 1.0.0\n",
    );
}

#[test]
fn a_wheel_without_metadata_is_refused() {
    let wheel = wheel_with(&[("demo/__init__.py", "value = 1\n")]);

    let error =
        metadata_from_wheel(&wheel, "demo-1.0.0-py3-none-any.whl").expect_err("nothing to read");

    assert!(error.contains("no dist-info METADATA"), "{error}");
}

/// A file named `METADATA` deeper in the archive, or one beside the
/// dist-info directory rather than in it, is not the metadata document.
#[test]
fn only_the_dist_info_metadata_counts() {
    let wheel = wheel_with(&[
        ("METADATA", "Name: impostor\nVersion: 9.9.9\n"),
        ("demo/nested/METADATA", "Name: impostor\nVersion: 9.9.9\n"),
        ("demo-1.0.0.dist-info/METADATA", "Name: demo\nVersion: 1.0.0\n"),
    ]);

    let document = metadata_from_wheel(&wheel, "demo-1.0.0-py3-none-any.whl").expect("metadata");

    assert_eq!(
        String::from_utf8(document).expect("metadata is text"),
        "Name: demo\nVersion: 1.0.0\n",
    );
}

#[test]
fn metadata_past_the_cap_is_refused_rather_than_cut_short() {
    let long = format!("Name: demo\nVersion: 1.0.0\n{}", "Requires-Dist: filler\n".repeat(500_000));
    let wheel = wheel_with(&[("demo-1.0.0.dist-info/METADATA", &long)]);

    let error =
        metadata_from_wheel(&wheel, "demo-1.0.0-py3-none-any.whl").expect_err("past the cap");

    assert!(error.contains("exceeds"), "{error}");
}

#[test]
fn an_index_url_gains_the_slash_a_project_page_resolves_against() {
    let url = index_url("https://example.test/simple").expect("index URL");

    assert_eq!(url.as_str(), "https://example.test/simple/");
    assert_eq!(
        url.join("demo/").expect("project page").as_str(),
        "https://example.test/simple/demo/",
    );
}

#[test]
fn an_index_url_that_could_carry_a_credential_is_refused() {
    for index in ["file:///etc/passwd", "https://user:secret@example.test/simple/"] {
        let error = index_url(index).expect_err("refused");
        assert!(error.contains("HTTP(S) URLs without embedded credentials"), "{index}: {error}");
    }
}

#[test]
fn a_full_budget_leaves_no_room_for_another_document() {
    assert!(within_budget(MAX_TOTAL_BYTES - 1));
    assert!(!within_budget(MAX_TOTAL_BYTES));
}
