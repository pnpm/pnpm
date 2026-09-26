//! pnpm/pnpm#3807: files materialized into `node_modules` follow the
//! umask of the install that writes them, not the one that populated the
//! store. A store entry written under `022` keeps its `0644`/`0755`
//! modes, so an install under `077` has to copy rather than link it.

#![cfg(unix)]

use crate::_utils::{ManifestDeps, write_project_manifest};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
};

/// Ships a shell script, so the store keeps it as an `-exec` entry.
const EXEC_DEP: (&str, &str) = ("@pnpm.e2e/sh-hello-world", "1.0.1");
const EXEC_FILE_REL: &str = concat!(
    "node_modules/.pnpm/@pnpm.e2e+sh-hello-world@1.0.1/",
    "node_modules/@pnpm.e2e/sh-hello-world/sh-hello-world.sh",
);
/// Ships a plain `package.json`, which the store keeps without the
/// `-exec` suffix.
const PLAIN_DEP: (&str, &str) = ("@pnpm.e2e/bravo-dep", "1.0.0");
const PLAIN_FILE_REL: &str = concat!(
    "node_modules/.pnpm/@pnpm.e2e+bravo-dep@1.0.0/",
    "node_modules/@pnpm.e2e/bravo-dep/package.json",
);

fn install_with_umask(dir: &Path, umask: libc::mode_t, args: &[&str]) {
    let mut command = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(dir)
        .without_ambient_pnpm_config();
    // SAFETY: `umask` is always safe to call, and this closure runs in the
    // forked child between `fork` and `exec`, where only async-signal-safe
    // operations are allowed and a syscall is one.
    unsafe {
        command.pre_exec(move || {
            libc::umask(umask);
            Ok(())
        });
    }
    let output = command
        .args(["install"])
        .args(args)
        .output()
        .expect("run pnpm install");
    assert!(
        output.status.success(),
        "install under umask {umask:#o} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn file_mode(path: &Path) -> u32 {
    fs::metadata(path)
        .unwrap_or_else(|error| panic!("stat {}: {error}", path.display()))
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn installs_materialize_files_at_the_current_umask_even_from_a_stale_store() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry {
        store_dir, cache_dir, mock_instance, ..
    } = npmrc_info;
    let deps = ManifestDeps { prod: &[EXEC_DEP, PLAIN_DEP], ..ManifestDeps::default() };
    write_project_manifest(&workspace, "root-a", deps);

    let other = root.path().join("other-project");
    fs::create_dir(&other).expect("create the second project directory");
    fs::write(
        other.join(".npmrc"),
        format!(
            "registry={}\nstore-dir={}\ncache-dir={}\n",
            mock_instance.url(),
            store_dir.display(),
            cache_dir.display(),
        ),
    )
    .expect("write the second project's .npmrc");
    fs::write(
        other.join("pnpm-workspace.yaml"),
        format!(
            "storeDir: {}\ncacheDir: {}\nenableGlobalVirtualStore: false\n",
            store_dir.display(),
            cache_dir.display(),
        ),
    )
    .expect("write the second project's pnpm-workspace.yaml");
    write_project_manifest(&other, "root-b", deps);

    // On APFS `auto` clones before it hardlinks, so the control project
    // pins the method that the inode assertion below can then check.
    install_with_umask(&workspace, 0o022, &["--package-import-method=hardlink"]);
    install_with_umask(&other, 0o077, &[]);

    assert_eq!(
        file_mode(&workspace.join(EXEC_FILE_REL)),
        0o755,
        "the store link matches the umask that wrote the store",
    );
    assert_eq!(
        file_mode(&workspace.join(PLAIN_FILE_REL)),
        0o644,
        "the store link matches the umask that wrote the store",
    );

    use std::os::unix::fs::MetadataExt;
    let inode = |path: &Path| fs::metadata(path).expect("stat").ino();

    let files_dir = store_dir.join(pnpm_store_dir::STORE_VERSION).join("files");
    let store_execs: Vec<PathBuf> = walkdir::WalkDir::new(&files_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with("-exec")
        })
        .map(walkdir::DirEntry::into_path)
        .collect();
    assert_eq!(store_execs.len(), 1, "only sh-hello-world ships an exec file");
    assert_eq!(file_mode(&store_execs[0]), 0o755, "the store keeps the umask that wrote it");

    let a_exec = workspace.join(EXEC_FILE_REL);
    let b_exec = other.join(EXEC_FILE_REL);
    assert_eq!(
        inode(&a_exec),
        inode(&store_execs[0]),
        "a matching-mode store file is still hardlinked",
    );
    assert_ne!(
        inode(&b_exec),
        inode(&store_execs[0]),
        "a store file written under another umask is not hardlinked into node_modules",
    );

    assert_eq!(
        file_mode(&b_exec),
        0o711,
        "an exec file from a 022-era store is materialized at 0o700 (`storeEntryMode` for the 077 \
         umask), then the bin shim's `ensure_executable_bits` adds the missing `x` bits",
    );
    assert_eq!(
        file_mode(&other.join(PLAIN_FILE_REL)),
        0o600,
        "a plain file from a 022-era store is copied at the current umask's entry mode",
    );

    drop(root);
}
