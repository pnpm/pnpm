use super::{find_rust_candidate, next_on_path};
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

    let Some(Candidate::RustToolchain { project_dir, request, .. }) = find_rust_candidate(&member)
    else {
        panic!("the project's toolchain file names a toolchain");
    };
    assert_eq!(project_dir, project);
    assert_eq!(request.channel.to_string(), "1.95.0");

    // rustup reads the nearest file, so one it handles itself stops the
    // search rather than letting a file further up decide.
    fs::write(project.join("rust-toolchain.toml"), "[toolchain]\npath = \"/opt/rust\"\n").unwrap();
    assert!(find_rust_candidate(&member).is_none());
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
        next_on_path("cargo", &shims, &path).map(|found| dunce::canonicalize(found).unwrap()),
        Some(dunce::canonicalize(rustup.join(&program)).unwrap()),
    );
    assert_eq!(next_on_path("cargo", &shims, &std::env::join_paths([&shims]).unwrap()), None);
}
