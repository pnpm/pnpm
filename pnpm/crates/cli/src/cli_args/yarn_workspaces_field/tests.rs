use super::{declares_yarn_workspaces, publish_new_workspace_manifest};
use crate::cli_args::package_manager::read_root_manifest_json;
use std::{fs, path::Path};

fn write_manifest(dir: &Path, contents: &str) {
    fs::write(dir.join("package.json"), contents).expect("write package.json");
}

fn declares_in(dir: &Path) -> bool {
    declares_yarn_workspaces(read_root_manifest_json(dir).as_ref())
}

#[test]
fn reports_a_manifest_declaring_workspace_patterns() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(dir.path(), r#"{"name":"x","workspaces":["packages/*"]}"#);
    assert!(declares_in(dir.path()));
}

/// An editor-written manifest may open with a UTF-8 BOM, which every other
/// manifest read in the CLI strips. The warning has to see the same field
/// those reads do.
#[test]
fn reports_workspace_patterns_through_a_utf8_bom() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(dir.path(), "\u{feff}{\"workspaces\":[\"packages/*\"]}");
    assert!(declares_in(dir.path()));
}

/// An empty array selects no project, so pnpm behaves the same with and
/// without it. pnpm 11 does not warn about it either.
#[test]
fn ignores_an_empty_workspaces_array() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_manifest(dir.path(), r#"{"workspaces":[]}"#);
    assert!(!declares_in(dir.path()));
}

/// Yarn's object spelling, and any other shape the field is given, is left
/// to the reader pnpm 11 has: neither version warns about it.
#[test]
fn tolerates_absent_malformed_and_non_array_manifests() {
    let dir = tempfile::tempdir().expect("create temp dir");
    assert!(!declares_in(dir.path()), "no manifest");

    write_manifest(dir.path(), "{ not json");
    assert!(!declares_in(dir.path()), "malformed manifest");

    write_manifest(dir.path(), r#"{"name":"x"}"#);
    assert!(!declares_in(dir.path()), "no workspaces field");

    write_manifest(dir.path(), r#"{"workspaces":{"packages":["packages/*"]}}"#);
    assert!(!declares_in(dir.path()), "object workspaces field");
}

fn files_besides_the_manifest(dir: &Path) -> Vec<fs::DirEntry> {
    fs::read_dir(dir)
        .expect("read dir")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name() != "pnpm-workspace.yaml")
        .collect()
}

#[test]
fn publishes_the_full_text_and_leaves_no_temp_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("pnpm-workspace.yaml");

    publish_new_workspace_manifest(&path, "packages:\n  - packages/*\n").expect("publish");

    assert_eq!(fs::read_to_string(&path).expect("read manifest"), "packages:\n  - packages/*\n");
    assert!(files_besides_the_manifest(dir.path()).is_empty());
}

#[test]
fn an_existing_manifest_is_left_untouched() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("pnpm-workspace.yaml");
    fs::write(&path, "packages:\n  - .\n").expect("author manifest");

    let error = publish_new_workspace_manifest(&path, "packages:\n  - packages/*\n")
        .expect_err("publish must not replace the manifest");

    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&path).expect("read manifest"), "packages:\n  - .\n");
    assert!(files_besides_the_manifest(dir.path()).is_empty());
}

#[cfg(unix)]
#[test]
fn the_published_manifest_gets_the_mode_of_a_newly_created_file() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("pnpm-workspace.yaml");
    let reference = dir.path().join("reference");
    fs::File::create(&reference).expect("create reference file");

    publish_new_workspace_manifest(&path, "packages:\n  - packages/*\n").expect("publish");

    let mode = |path: &Path| {
        fs::metadata(path)
            .expect("stat")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(&path), mode(&reference));
}

#[test]
fn concurrent_publishers_leave_one_complete_manifest() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("pnpm-workspace.yaml");
    let texts = ["packages:\n  - packages/*\n  - apps/web\n", "packages:\n  - crates/*\n"];

    let handles: Vec<_> = texts
        .iter()
        .map(|text| {
            let path = path.clone();
            let text = (*text).to_owned();
            std::thread::spawn(move || match publish_new_workspace_manifest(&path, &text) {
                Ok(()) => None,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    Some(fs::read_to_string(&path).expect("loser reads the manifest"))
                }
                Err(error) => panic!("unexpected publish error: {error}"),
            })
        })
        .collect();
    let mut losers_views = Vec::with_capacity(handles.len());
    for handle in handles {
        losers_views.push(handle.join().expect("publisher thread"));
    }

    let seen: Vec<_> = losers_views
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(seen.len(), 1, "exactly one publisher must lose");
    assert!(texts.contains(&seen[0].as_str()), "the loser saw a partial manifest: {:?}", seen[0]);
    let published = fs::read_to_string(&path).expect("read manifest");
    assert!(texts.contains(&published.as_str()), "partial manifest: {published:?}");
    assert!(files_besides_the_manifest(dir.path()).is_empty());
}
