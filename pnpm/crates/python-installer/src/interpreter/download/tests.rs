use super::{Bounds, Releases, ShasumsFileItem, builds_in, host_triple, within};
use crate::interpreter::VersionRequest;

/// A python-build-standalone `SHA256SUMS`, as the release writes it and
/// as the shared parser hands it back.
fn sums(files: &[String]) -> Vec<ShasumsFileItem> {
    use std::fmt::Write as _;
    let mut index = String::new();
    for file in files {
        writeln!(index, "{}  {file}", "a".repeat(64)).expect("writing to a String cannot fail");
    }
    pnpm_crypto_shasums_file::parse_shasums_file(&index)
}

fn built(versions: &[&str], triple: &str) -> Vec<String> {
    versions
        .iter()
        .map(|version| format!("cpython-{version}+20260901-{triple}-install_only_stripped.tar.gz"))
        .collect()
}

fn requires(specifiers: &str) -> pep440_rs::VersionSpecifiers {
    specifiers.parse().expect("requires-python fixture")
}

#[test]
fn the_index_offers_the_ordinary_interpreter_of_this_machine() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let mut files = built(&["3.13.15", "3.14.7"], &triple);
    // Other platforms, other builds of the same version, and the variants
    // a project asks for by name rather than by version.
    files.push(
        "cpython-3.13.15+20260901-riscv64-unknown-linux-gnu-install_only_stripped.tar.gz"
            .to_string(),
    );
    files.push(format!("cpython-3.13.15+20260901-{triple}-install_only.tar.gz"));
    files.push(format!("cpython-3.13.15+20260901-{triple}-debug-full.tar.zst"));
    files.extend(built(&["3.13.15"], &format!("{triple}-freethreaded")));
    let offered = builds_in(&sums(&files))
        .iter()
        .map(|build| build.version.to_string())
        .collect::<Vec<_>>();
    dbg!(&offered);
    assert_eq!(offered, ["3.13.15", "3.14.7"]);
}

/// A release index names what pnpm downloads and where it puts it.
#[test]
fn an_index_naming_a_path_rather_than_a_build_offers_nothing() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    for named in [
        "cpython-3.13.15+../../../elsewhere",
        "cpython-3.13.15+tag/../..",
        "cpython-3.13.15+",
        "cpython-3.13.15+2026-09-01",
    ] {
        let index = sums(&[format!("{named}-{triple}-install_only_stripped.tar.gz")]);
        assert!(builds_in(&index).is_empty(), "{named}");
    }
    let short = pnpm_crypto_shasums_file::parse_shasums_file(&format!(
        "aa  cpython-3.13.15+20260901-{triple}-install_only_stripped.tar.gz\n",
    ));
    assert!(builds_in(&short).is_empty());
}

#[test]
fn the_build_installed_is_the_newest_one_the_project_accepts() {
    let triple = host_triple().expect("these tests run where interpreters are built");
    let files = built(&["3.11.16", "3.12.14", "3.13.15", "3.14.7"], &triple);
    let releases = Releases { builds: builds_in(&sums(&files)) };

    assert_eq!(
        releases
            .best(None, None)
            .expect("the newest build")
            .version()
            .to_string(),
        "3.14.7",
    );
    let accepted = releases
        .best(Some(&requires(">=3.11,<3.14")), None)
        .expect("the newest build the range accepts");
    assert_eq!(accepted.version().to_string(), "3.13.15");
    assert_eq!(accepted.file, files[2]);
    assert_eq!(accepted.tag, "20260901");
    assert_eq!(accepted.integrity.to_string(), sums(&files)[2].integrity);
    assert_eq!(
        releases
            .best(None, Some(&VersionRequest::asking_for(&[3, 12])))
            .expect("the version the pin asks for")
            .version()
            .to_string(),
        "3.12.14",
    );
    assert!(
        releases
            .best(Some(&requires("==3.9.1")), None)
            .is_none(),
    );
}

/// What an archive holds is what it expands to and how many files that
/// is, neither of which is what a mirror had to send to hold it.
#[test]
fn an_archive_past_what_an_interpreter_is_never_reaches_the_store() {
    let archive = tempfile::NamedTempFile::new().expect("an archive");
    let mut written =
        tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for entry in 0..4 {
        let file = "an interpreter".repeat(8);
        let mut header = tar::Header::new_gnu();
        header.set_size(file.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        written
            .append_data(&mut header, format!("python/lib/{entry}"), file.as_bytes())
            .expect("an entry");
    }
    let compressed = written
        .into_inner()
        .expect("the archive")
        .finish()
        .expect("the archive");
    std::fs::write(archive.path(), &compressed).expect("write the archive");

    within(archive.path(), Bounds { bytes: 64 * 1024, entries: 4 })
        .expect("an archive within both bounds");

    let past_bytes = within(archive.path(), Bounds { bytes: 64, entries: 4 })
        .expect_err("an archive past the bytes it may unpack to");
    assert!(format!("{past_bytes:?}").contains("unpacks to more than"), "{past_bytes:?}");

    let past_entries = within(archive.path(), Bounds { bytes: 64 * 1024, entries: 3 })
        .expect_err("an archive past the entries it may hold");
    assert!(format!("{past_entries:?}").contains("more than 3 entries"), "{past_entries:?}");
}
