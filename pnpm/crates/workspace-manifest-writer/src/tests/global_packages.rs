use super::{TempDir, fs};

/// The global `config.yaml` that seeding it with `original` and writing
/// `entries` leaves, or `None` when there is no file.
fn run(original: Option<&str>, entries: &[(&str, bool)]) -> Option<String> {
    let dir = TempDir::new().expect("temp dir");
    let path = dir
        .path()
        .join("pnpm")
        .join("config.yaml");
    if let Some(text) = original {
        fs::create_dir_all(path.parent().expect("config dir")).expect("create config dir");
        fs::write(&path, text).expect("seed config.yaml");
    }
    crate::set_global_packages_build_permissions(&path, entries.iter().copied())
        .expect("update succeeds");
    fs::read_to_string(&path).ok()
}

#[test]
fn creates_the_config_file_and_its_directory() {
    let out = run(None, &[("esbuild", true), ("@scope/native", false)]);
    assert_eq!(
        out.as_deref(),
        Some(
            "globalPackagesConfig:\n  permissions:\n    '@scope/native':\n      build: false\n    esbuild:\n      build: true\n",
        ),
    );
}

#[test]
fn keeps_the_rest_of_the_file() {
    let out = run(
        Some("# machine settings\nstoreDir: ~/store # shared\ntrustPolicy: no-downgrade\n"),
        &[("esbuild", true)],
    );
    assert_eq!(
        out.as_deref(),
        Some(
            "globalPackagesConfig:\n  permissions:\n    esbuild:\n      build: true\n# machine settings\nstoreDir: ~/store # shared\ntrustPolicy: no-downgrade\n",
        ),
    );
}

#[test]
fn joins_an_existing_global_packages_config_section() {
    let out = run(
        Some("globalPackagesConfig:\n  trustPolicyExclude:\n    - vercel\n"),
        &[("esbuild", true)],
    );
    assert_eq!(
        out.as_deref(),
        Some(
            "globalPackagesConfig:\n  permissions:\n    esbuild:\n      build: true\n  trustPolicyExclude:\n    - vercel\n",
        ),
    );
}

#[test]
fn updates_and_adds_entries_of_an_existing_permissions_block() {
    let out = run(
        Some(
            "globalPackagesConfig:\n  permissions:\n    esbuild:\n      build: false # reviewed\n",
        ),
        &[("esbuild", true), ("sharp", false)],
    );
    assert_eq!(
        out.as_deref(),
        Some(
            "globalPackagesConfig:\n  permissions:\n    esbuild:\n      build: true # reviewed\n    sharp:\n      build: false\n",
        ),
    );
}

#[test]
fn writes_into_a_flow_style_permissions_block() {
    let out = run(
        Some("globalPackagesConfig:\n  permissions: { esbuild: { build: true } }\n"),
        &[("sharp", false)],
    );
    assert_eq!(
        out.as_deref(),
        Some(
            "globalPackagesConfig:\n  permissions: { esbuild: { build: true }, sharp: { build: false } }\n",
        ),
    );
}

#[test]
fn leaves_the_file_untouched_when_nothing_changes() {
    let original = "globalPackagesConfig:\n  permissions:\n    esbuild: { build: true }\n";
    let out = run(Some(original), &[("esbuild", true)]);
    assert_eq!(out.as_deref(), Some(original));
}

#[test]
fn refuses_a_name_with_a_control_character() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("config.yaml");
    let error =
        crate::set_global_packages_build_permissions(&path, [("evil\nstoreDir: /tmp", true)])
            .expect_err("a newline in a name is refused");
    assert!(
        matches!(error, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }),
        "{error:?}",
    );
    assert!(!path.exists());
}
