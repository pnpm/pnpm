use super::{Releases, builds_in, host_triple};
use crate::interpreter::VersionRequest;

/// A python-build-standalone `SHA256SUMS`, as the release writes it.
fn sums(files: &[String]) -> String {
    use std::fmt::Write as _;
    let mut index = String::new();
    for file in files {
        writeln!(index, "{}  {file}", "a".repeat(64)).expect("writing to a String cannot fail");
    }
    index
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
