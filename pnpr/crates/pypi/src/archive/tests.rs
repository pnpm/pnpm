use super::{DistributionKind, metadata_from_sdist, metadata_from_zip};
use std::io::{Cursor, Write};

fn zip_metadata(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in entries {
        archive
            .start_file::<_, ()>(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(content.as_bytes()).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

fn sdist(path: &str, content: &[u8]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append_data(&mut header, path, content).unwrap();
    archive
        .into_inner()
        .unwrap()
        .finish()
        .unwrap()
}

#[test]
fn reads_sdist_metadata_from_zip_and_tar() {
    let metadata = "Name: demo\nVersion: 1.0\n";
    assert_eq!(
        metadata_from_zip(
            &zip_metadata(&[("demo-1.0/PKG-INFO", metadata)]),
            "demo-1.0.zip",
            DistributionKind::Sdist
        )
        .unwrap(),
        metadata.as_bytes(),
    );
    assert_eq!(
        metadata_from_sdist(&sdist("demo-1.0/PKG-INFO", metadata.as_bytes()), "demo-1.0.tar.gz")
            .unwrap(),
        metadata.as_bytes(),
    );
    assert!(
        metadata_from_sdist(
            &sdist("demo-1.0/nested/PKG-INFO", metadata.as_bytes()),
            "demo-1.0.tar.gz"
        )
        .is_err(),
    );
    assert!(metadata_from_sdist(b"random bytes", "demo-1.0.tar.gz").is_err());
}

#[test]
fn refuses_missing_duplicate_and_oversize_metadata() {
    let filename = "demo-1.0-py3-none-any.whl";
    assert!(metadata_from_zip(b"random bytes", filename, DistributionKind::Wheel).is_err());
    assert!(
        metadata_from_zip(
            &zip_metadata(&[("METADATA", "Name: demo")]),
            filename,
            DistributionKind::Wheel
        )
        .is_err(),
    );
    assert!(
        metadata_from_zip(
            &zip_metadata(&[
                ("demo-1.0.dist-info/METADATA", "Name: demo"),
                ("other-1.0.dist-info/METADATA", "Name: other")
            ]),
            filename,
            DistributionKind::Wheel
        )
        .is_err(),
    );
    let large = "x".repeat(8 * 1024 * 1024 + 1);
    assert!(
        metadata_from_zip(
            &zip_metadata(&[("demo-1.0.dist-info/METADATA", &large)]),
            filename,
            DistributionKind::Wheel
        )
        .unwrap_err()
        .contains("exceeds"),
    );
}

#[test]
fn refuses_a_wheel_with_another_distributions_metadata_directory() {
    let bytes = zip_metadata(&[("other-1.0.dist-info/METADATA", "Name: demo\nVersion: 1.0\n")]);
    assert!(
        metadata_from_zip(&bytes, "demo-1.0-py3-none-any.whl", DistributionKind::Wheel)
            .unwrap_err()
            .contains("does not match"),
    );
}

#[test]
fn refuses_source_distributions_with_another_releases_root_directory() {
    let metadata = "Name: demo\nVersion: 1.0\n";
    assert!(
        metadata_from_sdist(&sdist("other-2.0/PKG-INFO", metadata.as_bytes()), "demo-1.0.tar.gz")
            .is_err(),
    );
    assert!(
        metadata_from_zip(
            &zip_metadata(&[("other-2.0/PKG-INFO", metadata)]),
            "demo-1.0.zip",
            DistributionKind::Sdist
        )
        .is_err(),
    );
}
