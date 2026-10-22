use super::{ACTIVE_WALKS, remove_pending_temp_files, track_temp_file};
use std::{
    fs,
    sync::{Mutex, atomic::Ordering},
};

/// The registry is process-global and `cargo test` runs these tests on
/// threads of one process, so a cleanup in one test could unlink a temp
/// file another test is still holding registered. Serialize the tests that
/// touch the registry. (The signal-handler path itself must stay lock-free;
/// this is test-only serialization.)
static LOCK: Mutex<()> = Mutex::new(());

#[test]
fn pending_temp_file_is_unlinked_by_cleanup() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join(".pnpm-lock.yaml.123.0.tmp");
    fs::write(&temp, "partial lockfile").expect("stage temp file");

    let _guard = track_temp_file(&temp);
    remove_pending_temp_files();

    assert!(!temp.exists(), "registered temp file should be unlinked: {temp:?}");
}

#[test]
fn released_temp_file_survives_cleanup() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join(".pnpm-lock.yaml.123.1.tmp");
    fs::write(&temp, "published").expect("stage temp file");

    drop(track_temp_file(&temp));
    remove_pending_temp_files();

    assert!(temp.exists(), "a released temp file belongs to the writer again: {temp:?}");
}

#[test]
fn released_slot_is_reused() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.tmp");
    let second = dir.path().join("second.tmp");
    fs::write(&first, "1").expect("stage first temp file");
    fs::write(&second, "2").expect("stage second temp file");

    drop(track_temp_file(&first));
    let _guard = track_temp_file(&second);
    remove_pending_temp_files();

    assert!(first.exists(), "released slot must not be unlinked: {first:?}");
    assert!(!second.exists(), "reused slot must be unlinked: {second:?}");
}

#[test]
fn slot_released_during_a_walk_stays_usable() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.tmp");
    let second = dir.path().join("second.tmp");
    fs::write(&first, "1").expect("stage first temp file");
    fs::write(&second, "2").expect("stage second temp file");

    // A walk in progress keeps the released path alive rather than freeing
    // it under the walker; the slot itself is free for the next write.
    ACTIVE_WALKS.fetch_add(1, Ordering::SeqCst);
    drop(track_temp_file(&first));
    ACTIVE_WALKS.fetch_sub(1, Ordering::SeqCst);
    let _guard = track_temp_file(&second);
    remove_pending_temp_files();

    assert!(first.exists(), "released slot must not be unlinked: {first:?}");
    assert!(!second.exists(), "reused slot must be unlinked: {second:?}");
}

#[cfg(unix)]
#[test]
fn die_from_signal_does_not_dump_core() {
    use std::{
        env, io,
        os::unix::process::{CommandExt, ExitStatusExt},
        path::Path,
        process::Command,
    };

    const CHILD_TEMP_ENV: &str = "PNPM_FS_DIE_FROM_SIGNAL_TEMP";
    if let Some(temp) = env::var_os(CHILD_TEMP_ENV) {
        let _guard = track_temp_file(Path::new(&temp));
        super::die_from_signal(libc::SIGABRT);
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join(".pnpm-lock.yaml.123.2.tmp");
    fs::write(&temp, "partial lockfile").expect("stage temp file");

    let mut command = Command::new(env::current_exe().expect("test binary path"));
    command
        .args(["--exact", "pending_temp::tests::die_from_signal_does_not_dump_core", "--nocapture"])
        .env(CHILD_TEMP_ENV, &temp);
    // SAFETY: `getrlimit`, `setrlimit`, and `prctl` are async-signal-safe and
    // use a stack local that outlives the call. Both attributes survive `exec`.
    unsafe {
        let mut inherited_limit: libc::rlimit = std::mem::zeroed();
        assert_eq!(
            libc::getrlimit(libc::RLIMIT_CORE, &raw mut inherited_limit),
            0,
            "getrlimit(RLIMIT_CORE) failed: {}",
            io::Error::last_os_error(),
        );
        assert_ne!(
            inherited_limit.rlim_max, 0,
            "core-dump regression test requires a non-zero RLIMIT_CORE hard limit",
        );
        command.pre_exec(|| {
            let mut limit: libc::rlimit = std::mem::zeroed();
            if libc::getrlimit(libc::RLIMIT_CORE, &raw mut limit) != 0 {
                return Err(io::Error::last_os_error());
            }
            limit.rlim_cur = limit.rlim_max;
            if libc::setrlimit(libc::RLIMIT_CORE, &raw const limit) != 0 {
                return Err(io::Error::last_os_error());
            }
            #[cfg(any(target_os = "linux", target_os = "android"))]
            if libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let status = command.status().expect("spawn child test process");

    assert!(!temp.exists(), "die_from_signal must unlink pending temp files: {temp:?}");
    assert_eq!(status.signal(), Some(libc::SIGABRT), "process should die by SIGABRT: {status:?}");
    assert!(!status.core_dumped(), "die_from_signal must not dump core: {status:?}");
}

/// The child half of
/// [`a_replaced_siginfo_handler_is_passed_the_signal_details`], which
/// spawns it.
///
/// Ignored so that no ordinary run reaches it, because it holds only in a
/// process of its own: it installs `earlier` before the `Once` behind
/// `install_temp_file_cleanup` is spent, which a sibling test that tracked
/// a temp file first would have spent already, and it leaves `SIGTERM`
/// pointing at pnpm's handler afterwards.
#[cfg(unix)]
#[test]
#[ignore = "spawned by a_replaced_siginfo_handler_is_passed_the_signal_details"]
fn forwards_a_delivery_to_the_handler_it_replaced() {
    use std::sync::atomic::{AtomicBool, AtomicI32};

    static REACHED_WITH: AtomicI32 = AtomicI32::new(0);
    static DETAILS_MATCHED: AtomicBool = AtomicBool::new(false);

    extern "C" fn earlier(
        signal: libc::c_int,
        siginfo: *mut libc::siginfo_t,
        _context: *mut libc::c_void,
    ) {
        REACHED_WITH.store(signal, Ordering::Release);
        // SAFETY: the kernel hands a handler installed with `SA_SIGINFO` a
        // valid `siginfo_t` for the delivery, and this reads one `c_int`
        // field of it without retaining the pointer.
        let matched = !siginfo.is_null() && unsafe { (*siginfo).si_signo } == signal;
        DETAILS_MATCHED.store(matched, Ordering::Release);
    }

    // SAFETY: an all-zero `sigaction` is a valid one to build on, and the
    // pointer is to a fully initialized stack local that outlives the call.
    // A null `oldact` discards the disposition being replaced.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_flags = libc::SA_SIGINFO;
        action.sa_sigaction = earlier as *const () as usize;
        libc::sigemptyset(&raw mut action.sa_mask);
        assert_eq!(libc::sigaction(libc::SIGTERM, &raw const action, std::ptr::null_mut()), 0);
    }

    // pnpm's handler goes on top, so `earlier` is the disposition it records
    // and the one it passes the signal on to.
    super::install_temp_file_cleanup();

    // SAFETY: `raise` delivers to this thread, where the disposition is
    // pnpm's handler over `earlier`, so neither ends the process and the
    // call returns.
    assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);

    assert_eq!(
        REACHED_WITH.load(Ordering::Acquire),
        libc::SIGTERM,
        "the signal was not passed on to the handler pnpm replaced",
    );
    assert!(
        DETAILS_MATCHED.load(Ordering::Acquire),
        "the replaced handler was called without the delivery's `siginfo_t`",
    );
}

/// Asserts on the `siginfo_t` the replaced handler received rather than
/// only on its being reached, because a wrong signature leaves those
/// arguments holding whatever the registers did.
///
/// The forwarding itself is
/// [`forwards_a_delivery_to_the_handler_it_replaced`], run as a child
/// process for the reasons given there.
#[cfg(unix)]
#[test]
fn a_replaced_siginfo_handler_is_passed_the_signal_details() {
    use std::{env, process::Command};

    const CHILD: &str = "pending_temp::tests::forwards_a_delivery_to_the_handler_it_replaced";

    let output = Command::new(env::current_exe().expect("test binary path"))
        .args(["--exact", CHILD, "--ignored", "--nocapture"])
        .output()
        .expect("spawn child test process");
    let report = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "{CHILD} failed:\n{report}");
    // `--exact` names the child in a string, so a rename would leave the
    // child running no test at all and still exiting zero.
    assert!(
        report.contains("1 passed"),
        "{CHILD} matched no test, so it has been renamed or gated out:\n{report}",
    );
}
