use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Canonicalize a path the same way the production CLI does. The CLI
/// runs `dunce::canonicalize` on `--dir` and threads that through to
/// `Config::current`, so on Windows the printed `storeDir` is the long
/// form (`C:\Users\runneradmin\...`) even when the surrounding test
/// runs in a `TEMP` directory whose env var resolves to the short DOS
/// form (`C:\Users\RUNNER~1\...`). Mirror that here so the expected
/// value matches what pacquet actually prints.
fn canonicalize(path: &Path) -> PathBuf {
    dunce::canonicalize(path).expect("canonicalize path")
}

#[test]
fn store_path_should_return_store_dir_from_pnpm_workspace_yaml() {
    // `storeDir` is a project-structural setting — in pnpm 11 (and now
    // pacquet) it's only honoured from `pnpm-workspace.yaml`, not `.npmrc`.
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    eprintln!("Creating pnpm-workspace.yaml...");
    fs::write(workspace.join("pnpm-workspace.yaml"), "storeDir: foo/bar\n")
        .expect("write to pnpm-workspace.yaml");

    eprintln!("Executing pacquet store path...");
    let output = pacquet.with_args(["store", "path"]).output().expect("run pacquet store path");
    dbg!(&output);

    eprintln!("Exit status code");
    assert!(output.status.success());

    eprintln!("Stdout");
    let normalize = |path: &str| path.replace('\\', "/");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end().pipe(normalize),
        canonicalize(&workspace).join("foo/bar").to_string_lossy().pipe_as_ref(normalize),
    );

    drop(root); // cleanup
}
