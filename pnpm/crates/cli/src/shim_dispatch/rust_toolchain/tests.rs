use super::{RustSelection, find_rust_candidate, next_on_path};
use crate::shim_dispatch::Candidate;
use std::{fs, path::Path};

#[test]
fn the_nearest_toolchain_file_decides() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let member = project.join("crates/member");
    fs::create_dir_all(&member).unwrap();
    fs::write(root.path().join("rust-toolchain.toml"), "1.94.0").unwrap();
    fs::write(project.join("rust-toolchain.toml"), "1.95.0").unwrap();

    let RustSelection::Managed(Candidate::RustToolchain { project_dir, request, .. }) =
        find_rust_candidate(&member, None)
    else {
        panic!("the project's toolchain file names a toolchain");
    };
    assert_eq!(project_dir, project);
    assert_eq!(request.channel.to_string(), "1.95.0");

    // rustup reads the nearest file, so one it handles itself stops the
    // search rather than letting a file further up decide.
    fs::write(project.join("rust-toolchain.toml"), "[toolchain]\npath = \"/opt/rust\"\n").unwrap();
    assert!(matches!(find_rust_candidate(&member, None), RustSelection::Rustup));

    fs::remove_file(project.join("rust-toolchain.toml")).unwrap();
    fs::remove_file(root.path().join("rust-toolchain.toml")).unwrap();
    assert!(matches!(find_rust_candidate(&member, None), RustSelection::Unpinned));
}

#[test]
fn the_next_program_on_path_skips_the_shim_directory() {
    let root = tempfile::tempdir().unwrap();
    let shims = root.path().join("shims");
    let rustup = root.path().join("rustup");
    let program = format!("cargo{}", std::env::consts::EXE_SUFFIX);
    for dir in [&shims, &rustup] {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(&program), "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(dir.join(&program), fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let path = std::env::join_paths([Path::new("relative"), &shims, &rustup]).unwrap();

    assert_eq!(
        next_on_path("cargo", &shims, &shims.join(&program), &path)
            .map(|found| dunce::canonicalize(found).unwrap()),
        Some(dunce::canonicalize(rustup.join(&program)).unwrap()),
    );
    assert_eq!(
        next_on_path(
            "cargo",
            &shims,
            &shims.join(&program),
            &std::env::join_paths([&shims]).unwrap()
        ),
        None,
    );

    // A link back to the shim elsewhere on PATH is passed over too.
    let linked = root.path().join("linked");
    fs::create_dir_all(&linked).unwrap();
    fs::hard_link(shims.join(&program), linked.join(&program)).unwrap();
    let path = std::env::join_paths([&linked, &rustup]).unwrap();
    assert_eq!(
        next_on_path("cargo", &shims, &shims.join(&program), &path)
            .map(|found| dunce::canonicalize(found).unwrap()),
        Some(dunce::canonicalize(rustup.join(&program)).unwrap()),
    );
}

#[test]
fn a_rustup_directory_override_at_or_below_the_file_decides() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let member = project.join("crates/member");
    fs::create_dir_all(&member).unwrap();
    fs::write(project.join("rust-toolchain.toml"), "1.95.0").unwrap();
    let settings = root.path().join("settings.toml");
    let write_override = |dir: &Path| {
        let mut overrides = toml::Table::new();
        overrides.insert(dir.to_str().unwrap().to_string(), "nightly".into());
        let mut table = toml::Table::new();
        table.insert("overrides".to_string(), overrides.into());
        fs::write(&settings, table.to_string()).unwrap();
    };

    write_override(root.path());
    assert!(matches!(find_rust_candidate(&member, Some(&settings)), RustSelection::Managed(_)));

    write_override(&member);
    assert!(matches!(find_rust_candidate(&member, Some(&settings)), RustSelection::Rustup));

    write_override(&project);
    assert!(matches!(find_rust_candidate(&member, Some(&settings)), RustSelection::Rustup));
}

#[test]
fn a_rustup_directory_override_without_a_toolchain_file_is_rustups() {
    let root = tempfile::tempdir().unwrap();
    let settings = root.path().join("settings.toml");
    let mut overrides = toml::Table::new();
    overrides.insert(
        root.path()
            .to_str()
            .unwrap()
            .to_string(),
        "nightly".into(),
    );
    let mut table = toml::Table::new();
    table.insert("overrides".to_string(), overrides.into());
    fs::write(&settings, table.to_string()).unwrap();

    assert!(matches!(find_rust_candidate(root.path(), Some(&settings)), RustSelection::Rustup));
}
