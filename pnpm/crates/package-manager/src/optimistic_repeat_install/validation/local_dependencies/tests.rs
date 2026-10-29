use super::{FileDigests, Validation, changed, record};
use pnpm_workspace_state::{FileSignature, LocalDependencies};
use std::{collections::BTreeMap, fs};

fn tarballs(paths: &[&str]) -> LocalDependencies {
    LocalDependencies {
        tarballs: paths
            .iter()
            .map(|path| (*path).to_string())
            .collect(),
        directories: Vec::new(),
    }
}

fn signature(digest: &str) -> FileSignature {
    FileSignature { mtime_ns: 0, ctime_ns: None, size: 0, digest: digest.to_string() }
}

#[test]
fn carries_the_entries_of_lockfiles_the_projects_still_name() {
    let previous = Validation {
        files: BTreeMap::from([
            ("/kept.tgz".to_string(), signature("kept")),
            ("/dropped.tgz".to_string(), signature("dropped")),
        ]),
        local_dependencies: BTreeMap::from([
            ("other".to_string(), tarballs(&["/kept.tgz"])),
            ("stale".to_string(), tarballs(&["/dropped.tgz"])),
        ]),
        ..Validation::default()
    };
    let mut validation = Validation {
        lockfile: Some("current".to_string()),
        lockfiles: BTreeMap::from([("packages/b".to_string(), Some("other".to_string()))]),
        ..Validation::default()
    };
    let digests = FileDigests::new(None);
    record(&mut validation, None, (Some(&previous), &digests), |signature| signature);
    dbg!(&validation);
    assert_eq!(
        validation.local_dependencies,
        BTreeMap::from([("other".to_string(), tarballs(&["/kept.tgz"]))]),
    );
    assert_eq!(validation.files.get("/kept.tgz"), Some(&signature("kept")));
    assert!(!validation.files.contains_key("/dropped.tgz"));
}

#[test]
fn reports_a_replaced_tarball_and_any_directory() {
    let dir = tempfile::tempdir().expect("create a temp dir");
    let tarball = dir.path().join("pkg.tgz");
    fs::write(&tarball, "first").expect("write the tarball");
    let tarball = tarball.to_string_lossy().into_owned();
    let mut validation =
        Validation { lockfile: Some("current".to_string()), ..Validation::default() };
    let digests = FileDigests::new(None);
    record(
        &mut validation,
        Some(("current", tarballs(&[&tarball]))),
        (None, &digests),
        |signature| signature,
    );
    assert!(!changed(&validation));

    fs::write(&tarball, "second").expect("replace the tarball");
    assert!(changed(&validation));

    let with_directory = Validation {
        local_dependencies: BTreeMap::from([(
            "current".to_string(),
            LocalDependencies { tarballs: Vec::new(), directories: vec!["/lib".to_string()] },
        )]),
        ..Validation::default()
    };
    assert!(changed(&with_directory));
}

#[test]
fn keeps_the_entry_of_a_lockfile_without_local_dependencies() {
    let mut validation =
        Validation { lockfile: Some("current".to_string()), ..Validation::default() };
    let digests = FileDigests::new(None);
    record(
        &mut validation,
        Some(("current", LocalDependencies::default())),
        (None, &digests),
        |signature| signature,
    );
    assert_eq!(
        validation.local_dependencies,
        BTreeMap::from([("current".to_string(), LocalDependencies::default())]),
    );
    assert!(!changed(&validation));
}
