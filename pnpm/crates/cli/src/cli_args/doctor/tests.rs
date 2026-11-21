mod security;

use super::{
    CheckResult,
    CheckStatus,
    DoctorArgs,
    DoctorReport,
    check_versions,
    filesystem::{
        ProbeWorkspaces,
        check_filesystem_capabilities,
        check_filesystem_capabilities_in_workspaces,
        probe_link_capabilities,
    },
    install_probe::last_message,
    probe_path::{
        ProbeDir,
        can_write_to_dir,
    },
    render_report,
    status_mark,
};
use pnpm_config::{
    Config,
    NodeLinker,
    PNPM_VERSION,
};
use pnpm_fs::{
    FsHardLink,
    FsReflink,
    Host,
};
use pnpm_store_dir::StoreDir;
use pretty_assertions::assert_eq;
use std::{
    fs,
    io,
    path::{
        Component,
        Path,
    },
};

fn report(checks: Vec<CheckResult>) -> DoctorReport {
    DoctorReport { checks }
}

/// Simulates independent filesystem mounts under a single test sandbox: each
/// top-level `mount-*` directory acts as its own device, so linking within one
/// mount delegates to [`Host`] while linking across two mounts fails with
/// [`io::ErrorKind::CrossesDevices`].
struct CrossMountFs;

fn simulated_mount_name(path: &Path) -> Option<&std::ffi::OsStr> {
    path.components()
        .find_map(|component| match component {
            Component::Normal(name) if name.to_string_lossy().starts_with("mount-") => Some(name),
            _ => None,
        })
}

fn ensure_same_mount(source: &Path, target: &Path) -> io::Result<()> {
    if simulated_mount_name(source) != simulated_mount_name(target) {
        return Err(io::Error::new(
            io::ErrorKind::CrossesDevices,
            "cross-device link not permitted",
        ));
    }
    Ok(())
}

impl FsReflink for CrossMountFs {
    fn reflink(source: &Path, target: &Path) -> io::Result<()> {
        ensure_same_mount(source, target)?;
        Host::reflink(source, target)
    }
}

impl FsHardLink for CrossMountFs {
    fn hard_link(source: &Path, target: &Path) -> io::Result<()> {
        ensure_same_mount(source, target)?;
        Host::hard_link(source, target)
    }
}

/// The registry answers slower than `fetchTimeout` but well within the
/// doctor's own timeout, so the check passes only if it ignores `fetchTimeout`.
#[tokio::test]
async fn registry_connectivity_ignores_fetch_timeout() {
    let mut server = mockito::Server::new_async().await;
    let _ping = server
        .mock("GET", mockito::Matcher::Any)
        .with_status(200)
        .with_chunked_body(|writer| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            writer.write_all(b"{}")
        })
        .create_async()
        .await;
    let config =
        Config { registry: format!("{}/", server.url()), fetch_timeout: 50, ..Config::default() };
    let doctor = DoctorArgs { offline: false, benchmark: false, json: false };

    let result = doctor.check_connectivity(&config).await;
    dbg!(&result);
    assert_eq!(result.status, CheckStatus::Pass);
}

#[test]
fn render_report_summarizes_a_clean_run() {
    let output = render_report(&report(vec![CheckResult::pass("Versions", "pnpm 12.0.0")]));
    dbg!(&output);
    assert_eq!(output, "✓ Versions: pnpm 12.0.0\n\nAll checks passed");
}

/// A warning must not read as a failure, and its fix has to reach the user,
/// a check nobody can act on is noise.
#[test]
fn render_report_shows_the_fix_for_a_warning() {
    let output =
        render_report(&report(vec![CheckResult::warn("Filesystem", "only copying", "Move it.")]));
    dbg!(&output);
    assert_eq!(
        output,
        "‼ Filesystem: only copying\n    Move it.\n\nAll checks passed with 1 warning(s)",
    );
}

#[test]
fn render_report_counts_failures() {
    let output = render_report(&report(vec![
        CheckResult::pass("Versions", "pnpm 12.0.0"),
        CheckResult::fail("Store directory", "no write access to /nope", "Fix it."),
    ]));
    dbg!(&output);
    assert!(output.ends_with("1 check(s) failed"), "{output}");
}

#[test]
fn status_marks_are_distinct() {
    assert_eq!(status_mark(CheckStatus::Pass), "✓");
    assert_eq!(status_mark(CheckStatus::Warn), "‼");
    assert_eq!(status_mark(CheckStatus::Fail), "✗");
}

/// The JSON shape is what the release pipeline and any other tooling read, so
/// it is a contract: camelCase keys, and absent fields omitted rather than null.
#[test]
fn json_report_uses_camel_case_and_omits_empty_fields() {
    let mut check = CheckResult::pass("Filesystem", "available: hardlink");
    check.duration_ms = Some(3);
    let json = serde_json::to_string(&report(vec![check])).expect("serialize report");
    dbg!(&json);
    assert_eq!(
        json,
        r#"{"checks":[{"title":"Filesystem","status":"pass","detail":"available: hardlink","durationMs":3}]}"#,
    );
}

#[test]
fn probe_reports_the_links_a_normal_filesystem_supports() {
    let store = tempfile::tempdir().expect("create store temp dir");
    let target = tempfile::tempdir().expect("create target temp dir");
    let store_canonical = dunce::canonicalize(store.path()).expect("canonicalize store");
    let target_canonical = dunce::canonicalize(target.path()).expect("canonicalize target");
    let capabilities = {
        let mut workspaces = ProbeWorkspaces {
            store: ProbeDir::new(&store_canonical).expect("create store probe"),
            link_target: ProbeDir::new(&target_canonical).expect("create target probe"),
            symlink_target: None,
        };
        probe_link_capabilities::<Host>(&mut workspaces).expect("probe links")
    };
    dbg!(&capabilities);
    assert!(capabilities.hardlink, "a temp dir must support hardlinks");
    #[cfg(unix)]
    assert!(capabilities.symlink, "a Unix temp dir must support symlinks");
    assert_eq!(fs::read_dir(&store_canonical).expect("read store").count(), 0);
    assert_eq!(fs::read_dir(&target_canonical).expect("read target").count(), 0);
}

#[test]
fn can_write_to_dir_detects_a_writable_dir() {
    let dir = tempfile::tempdir().expect("create temp dir");
    assert!(can_write_to_dir(dir.path()));
    assert_eq!(
        fs::read_dir(dir.path()).expect("read temp dir").count(),
        0,
        "file-write probe must clean up its temporary file",
    );
}

#[test]
fn can_write_to_dir_accepts_a_missing_nested_dir_without_creating_it() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let missing = dir.path().join("cache/pnpm/dlx");
    assert!(can_write_to_dir(&missing));
    assert!(
        !dir.path().join("cache").exists(),
        "probing writability must not leave missing directories behind",
    );
}

#[test]
fn can_write_to_dir_rejects_empty_and_file_blocked_paths() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let blocker = dir.path().join("not-a-directory");
    fs::write(&blocker, "file").expect("write blocking file");

    assert!(!can_write_to_dir(Path::new("")));
    assert!(!can_write_to_dir(&blocker));
    assert!(!can_write_to_dir(&blocker.join("cache/pnpm")));
}

#[cfg(any(unix, windows))]
#[test]
fn can_write_to_dir_rejects_dangling_symlinks() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let dangling = dir.path().join("dangling-link");
    let missing_target = dir.path().join("nowhere");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&missing_target, &dangling).expect("create dangling symlink");
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_dir(&missing_target, &dangling) {
        if error.raw_os_error() == Some(1314) {
            eprintln!("skipping symlink assertion without Windows symlink privilege: {error}");
            return;
        }
        panic!("create dangling symlink: {error}");
    }
    assert!(!can_write_to_dir(&dangling));
    assert!(!can_write_to_dir(&dangling.join("pnpm")));
}

#[cfg(unix)]
#[test]
fn can_write_to_dir_rejects_readonly_ancestors() {
    use std::os::unix::fs::PermissionsExt as _;

    // Root bypasses POSIX permission bits.
    if pnpm_ffi_process::effective_user_id() == 0 {
        return;
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let readonly = dir.path().join("readonly");
    fs::create_dir(&readonly).expect("create readonly dir");
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o555)).expect("chmod 0555");
    let direct = can_write_to_dir(&readonly);
    let nested = can_write_to_dir(&readonly.join("missing/store/v11"));
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o755)).expect("restore permissions");

    assert!(!direct);
    assert!(!nested);
}

#[test]
fn filesystem_check_passes_before_store_or_modules_dirs_exist() {
    let sandbox = tempfile::tempdir().expect("create sandbox");
    let project_dir = sandbox.path().join("proj");
    fs::create_dir(&project_dir).expect("create project dir");
    let missing_store_parent = sandbox.path().join("fresh-data/pnpm/store");
    let modules_dir = project_dir.join("node_modules");

    let mut config = Config {
        store_dir: StoreDir::new(&missing_store_parent),
        modules_dir: modules_dir.clone(),
        install_state_dir: modules_dir.join(".pnpm"),
        ..Config::default()
    };
    config.apply_global_virtual_store_derivation(false, false);

    let result = check_filesystem_capabilities::<Host>(&config);
    dbg!(&result);
    assert_eq!(result.status, CheckStatus::Pass);
    assert!(
        !sandbox
            .path()
            .join("fresh-data")
            .exists(),
        "store parent must stay uncreated",
    );
    assert!(!modules_dir.exists(), "node_modules must stay uncreated");
    assert_eq!(
        fs::read_dir(&project_dir).expect("read project dir").count(),
        0,
        "temporary probe directory in project must be cleaned up",
    );
}

#[test]
fn filesystem_check_warns_when_store_and_project_are_on_different_mounts() {
    let sandbox = tempfile::tempdir().expect("create sandbox");
    let mount_a = sandbox.path().join("mount-a");
    let mount_b = sandbox.path().join("mount-b");
    let project_dir = mount_b.join("proj");
    fs::create_dir_all(&mount_a).expect("create mount-a");
    fs::create_dir_all(&project_dir).expect("create mount-b project");

    let modules_dir = project_dir.join("node_modules");
    let mut config = Config {
        store_dir: StoreDir::new(mount_a.join("store")),
        modules_dir: modules_dir.clone(),
        install_state_dir: modules_dir.join(".pnpm"),
        ..Config::default()
    };
    config.apply_global_virtual_store_derivation(false, false);

    let result = check_filesystem_capabilities::<CrossMountFs>(&config);
    dbg!(&result);
    assert_eq!(result.status, CheckStatus::Warn);
    #[cfg(unix)]
    assert_eq!(result.detail.as_deref(), Some("available: symlink"));
    assert_eq!(
        result.fix.as_deref(),
        Some(
            "Neither reflink nor hardlink works between the store and this project; installs will copy files and be slower. Put the store on the same filesystem as your projects."
        ),
    );
}

#[test]
fn filesystem_check_passes_across_mounts_when_global_virtual_store_links_on_store_mount() {
    let sandbox = tempfile::tempdir().expect("create sandbox");
    let mount_a = sandbox.path().join("mount-a");
    let mount_b = sandbox.path().join("mount-b");
    let project_dir = mount_b.join("proj");
    fs::create_dir_all(&mount_a).expect("create mount-a");
    fs::create_dir_all(&project_dir).expect("create mount-b project");

    let modules_dir = project_dir.join("node_modules");
    let mut config = Config {
        store_dir: StoreDir::new(mount_a.join("store")),
        modules_dir,
        enable_global_virtual_store: true,
        ..Config::default()
    };
    config.apply_global_virtual_store_derivation(false, false);

    let result = check_filesystem_capabilities::<CrossMountFs>(&config);
    dbg!(&result);
    assert_eq!(result.status, CheckStatus::Pass);
}

#[test]
fn filesystem_check_warns_for_hoisted_cross_mount_modules_dir() {
    let sandbox = tempfile::tempdir().expect("create sandbox");
    let mount_a = sandbox.path().join("mount-a");
    let project_dir = sandbox.path().join("mount-b/proj");
    fs::create_dir_all(&mount_a).expect("create mount-a");
    fs::create_dir_all(&project_dir).expect("create mount-b project");

    let config = Config {
        store_dir: StoreDir::new(mount_a.join("store")),
        modules_dir: project_dir.join("node_modules"),
        node_linker: NodeLinker::Hoisted,
        ..Config::default()
    };

    let result = check_filesystem_capabilities::<CrossMountFs>(&config);
    dbg!(&result);
    assert_eq!(result.status, CheckStatus::Warn);
}

#[test]
fn filesystem_check_warns_when_store_probe_cannot_write_file() {
    let sandbox = tempfile::tempdir().expect("create temp dir");
    let canonical = dunce::canonicalize(sandbox.path()).expect("canonicalize sandbox");
    let result = {
        let mut workspaces = ProbeWorkspaces {
            store: ProbeDir::new(&canonical).expect("create store probe"),
            link_target: ProbeDir::new(&canonical).expect("create target probe"),
            symlink_target: None,
        };
        workspaces.store.invalidate_for_test();
        check_filesystem_capabilities_in_workspaces::<Host>(&mut workspaces)
    };
    dbg!(&result);
    assert_eq!(result.status, CheckStatus::Warn);
    assert_eq!(result.detail.as_deref(), Some("could not write a probe file"));
    assert_eq!(result.fix.as_deref(), Some("Check that the store directory is writable."));
    assert_eq!(fs::read_dir(&canonical).expect("read sandbox").count(), 0);
}

/// The install smoke test surfaces the last meaningful stderr line, so a
/// trailing blank line must not swallow the actual error.
#[test]
fn last_message_skips_trailing_blanks() {
    assert_eq!(last_message("first\nERR_PNPM_BROKEN  it broke\n\n"), "ERR_PNPM_BROKEN  it broke");
    assert_eq!(last_message(""), "");
}

#[test]
fn last_message_joins_a_wrapped_error_cause() {
    let stderr = "Error: ERR_PNPM_EXECUTOR_LIFECYCLE_SCRIPT_FAILED\n\n  × installing dependencies\n  ╰─▶ /p postinstall: `probe` exited with exit\n      status: 127\n\n";

    assert_eq!(last_message(stderr), "/p postinstall: `probe` exited with exit status: 127");
    assert_eq!(last_message("plain failure\n"), "plain failure");
    assert_eq!(last_message(""), "");
}

/// The `cli` crate carries a placeholder `Cargo.toml` version, so the check
/// has to report the released version that `pnpm --version` prints.
#[test]
fn check_versions_reports_the_released_pnpm_version() {
    let detail = check_versions().detail.expect("versions detail");
    assert!(detail.starts_with(&format!("pnpm {PNPM_VERSION}")), "{detail}");
}
