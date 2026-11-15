use super::import_registries;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

/// A stand-in `pnpr` that records its arguments and one variable, one per
/// line, and exits with `status`.
fn recording_binary(dir: &Path, status: u8) -> PathBuf {
    let binary = dir.join("pnpr");
    let record = dir.join("record");
    fs::write(
        &binary,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" \"env=$PNPR_BENCHMARK_PROBE\" > '{}'\necho refused >&2\nexit {status}\n",
            record.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    binary
}

#[test]
fn import_runs_the_serve_command_with_the_registries_subcommand() {
    let dir = tempfile::tempdir().unwrap();
    let mut serve = Command::new(recording_binary(dir.path(), 0));
    serve
        .args(["--config", "pnpr.yaml", "--storage", "storage", "--public-url", "http://h:1"])
        .env("PNPR_BENCHMARK_PROBE", "kept");
    import_registries(&serve, Path::new("registries.yaml"));
    let record = fs::read_to_string(dir.path().join("record")).unwrap();
    assert_eq!(
        record.lines().collect::<Vec<_>>(),
        [
            "--config",
            "pnpr.yaml",
            "--storage",
            "storage",
            "--public-url",
            "http://h:1",
            "registries",
            "import",
            "registries.yaml",
            "env=kept",
        ],
    );
}

#[test]
#[should_panic(expected = "refused")]
fn a_failed_import_panics_with_its_output() {
    let dir = tempfile::tempdir().unwrap();
    import_registries(&Command::new(recording_binary(dir.path(), 1)), Path::new("registries.yaml"));
}
