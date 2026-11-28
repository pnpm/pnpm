#![allow(
    clippy::disallowed_methods,
    reason = "test setup starts processes the executor does not run"
)]
use super::{
    PendingTempFile,
    PendingTempFiles,
    remove_pending_temp_files,
    track_temp_file,
};
#[cfg(unix)]
use command_extra::CommandExtra;
use std::{
    fs,
    io,
    path::Path,
    sync::{
        Arc,
        Mutex,
        atomic::{
            AtomicBool,
            Ordering,
        },
        mpsc,
    },
    thread,
    time::{
        Duration,
        Instant,
    },
};

/// The registry is process-global and `cargo test` runs these tests on
/// threads of one process, so a cleanup in one test could unlink a temp
/// file another test is still holding registered. Serialize the tests that
/// touch the registry. (The signal-handler path itself must stay lock-free;
/// this is test-only serialization.)
static LOCK: Mutex<()> = Mutex::new(());

/// Register a temp file the test has already staged.
fn track_staged(path: &Path) -> PendingTempFile {
    let (guard, ()) = track_temp_file(path, || Ok(())).expect("track the staged temp file");
    guard
}

#[test]
fn pending_temp_file_is_unlinked_by_cleanup() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join(".pnpm-lock.yaml.123.0.tmp");
    fs::write(&temp, "partial lockfile").expect("stage temp file");

    let _guard = track_staged(&temp);
    remove_pending_temp_files();

    assert!(!temp.exists(), "registered temp file should be unlinked: {temp:?}");
}

#[test]
fn released_temp_file_survives_cleanup() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join(".pnpm-lock.yaml.123.1.tmp");
    fs::write(&temp, "published").expect("stage temp file");

    drop(track_staged(&temp));
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

    drop(track_staged(&first));
    let _guard = track_staged(&second);
    remove_pending_temp_files();

    assert!(first.exists(), "released slot must not be unlinked: {first:?}");
    assert!(!second.exists(), "reused slot must be unlinked: {second:?}");
}

/// A write that publishes its temp file while a cleanup walk is running
/// releases its slot from inside that walk, which is the shape the signal
/// handler creates. The slot has to come back usable afterwards.
#[test]
fn a_slot_released_during_a_cleanup_stays_usable() {
    let _lock = LOCK.lock().expect("registry test lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let first = dir.path().join("first.tmp");
    let second = dir.path().join("second.tmp");
    fs::write(&first, "1").expect("stage first temp file");
    fs::write(&second, "2").expect("stage second temp file");

    let mut guard = Some(track_staged(&first));
    remove_pending_temp_files();
    guard.take();

    let _guard = track_staged(&second);
    remove_pending_temp_files();

    assert!(!first.exists(), "a registered temp file is unlinked: {first:?}");
    assert!(!second.exists(), "the reused slot is unlinked: {second:?}");
}

#[cfg(unix)]
#[test]
fn die_from_signal_does_not_dump_core() {
    use std::{
        env,
        os::unix::process::ExitStatusExt,
        process::Command,
    };

    const CHILD_TEMP_ENV: &str = "PNPM_FS_DIE_FROM_SIGNAL_TEMP";
    if let Some(temp) = env::var_os(CHILD_TEMP_ENV) {
        let _guard = track_staged(Path::new(&temp));
        super::die_from_signal(libc::SIGABRT);
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join(".pnpm-lock.yaml.123.2.tmp");
    fs::write(&temp, "partial lockfile").expect("stage temp file");

    let mut command = Command::new(env::current_exe().expect("test binary path"))
        .with_args([
            "--exact",
            "pending_temp::tests::die_from_signal_does_not_dump_core",
            "--nocapture",
        ])
        .with_env(CHILD_TEMP_ENV, &temp);
    pnpm_testing_utils::spawn::enable_core_dumps(&mut command);

    let status = command.status().expect("spawn child test process");

    assert!(!temp.exists(), "die_from_signal must unlink pending temp files: {temp:?}");
    assert_eq!(status.signal(), Some(libc::SIGABRT), "process should die by SIGABRT: {status:?}");
    assert!(!status.core_dumped(), "die_from_signal must not dump core: {status:?}");
}

/// Wait until `done` holds, failing the test after 30 seconds.
fn wait_for(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "{what} did not happen in 30s");
        thread::sleep(Duration::from_millis(1));
    }
}

/// A write that passed the gate before the cleanup closed it creates its
/// temp file after the cleanup has started. The cleanup waits for that
/// creation and only then walks, so the temp file is unlinked.
#[test]
fn the_exit_cleanup_waits_for_a_temp_file_being_created() {
    static FILES: PendingTempFiles = PendingTempFiles::new(Duration::from_secs(60));
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join("staged.tmp");
    let (entered, entered_receiver) = mpsc::channel();
    let (release, release_receiver) = mpsc::channel::<()>();
    let writer = thread::spawn({
        let temp = temp.clone();
        move || {
            FILES.track(&temp, || {
                entered
                    .send(())
                    .expect("report the create");
                release_receiver.recv().expect("wait for the release");
                fs::write(&temp, "partial")
            })
        }
    });
    entered_receiver.recv().expect("the writer enters its create");

    let cleanup = thread::spawn(|| FILES.remove_before_exit());
    wait_for("closing the gate", || FILES.exiting.load(Ordering::SeqCst));
    thread::sleep(Duration::from_millis(50));
    assert!(!cleanup.is_finished(), "the cleanup must wait for the create in flight");

    release
        .send(())
        .expect("release the writer");
    let (guard, ()) = writer
        .join()
        .expect("join the writer")
        .expect("the writer creates its temp file");
    cleanup.join().expect("join the cleanup");
    assert!(!temp.exists(), "the temp file created during the cleanup is unlinked: {temp:?}");
    drop(guard);
}

/// A write that reaches the gate after the cleanup closed it never runs its
/// create, so it leaves no temp file for the cleanup to miss.
#[test]
fn a_write_after_the_exit_cleanup_never_creates_its_temp_file() {
    static FILES: PendingTempFiles = PendingTempFiles::new(Duration::from_secs(60));
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join("late.tmp");
    FILES.remove_before_exit();

    let created = Arc::new(AtomicBool::new(false));
    // The writer blocks for good, as it would until the process ended, so
    // its thread is left behind rather than joined.
    drop(thread::spawn({
        let temp = temp.clone();
        let created = Arc::clone(&created);
        move || {
            FILES.track(&temp, || {
                created.store(true, Ordering::SeqCst);
                fs::write(&temp, "late")
            })
        }
    }));

    thread::sleep(Duration::from_millis(100));
    assert!(!created.load(Ordering::SeqCst), "a write after the cleanup must not create");
    assert!(!temp.exists(), "no temp file appears after the cleanup: {temp:?}");
}

/// A create that does not finish, like one the signal interrupted on the
/// handler's own thread, holds the cleanup only until the limit.
#[test]
fn the_exit_cleanup_stops_waiting_at_the_limit() {
    static FILES: PendingTempFiles = PendingTempFiles::new(Duration::from_millis(20));
    let dir = tempfile::tempdir().expect("tempdir");
    let temp = dir.path().join("stuck.tmp");
    let (entered, entered_receiver) = mpsc::channel();
    let (release, release_receiver) = mpsc::channel::<()>();
    let writer = thread::spawn(move || {
        FILES.track(&temp, || {
            entered
                .send(())
                .expect("report the create");
            release_receiver.recv().expect("wait for the release");
            Ok(())
        })
    });
    entered_receiver.recv().expect("the writer enters its create");

    let cleanup = thread::spawn(|| FILES.remove_before_exit());
    wait_for("the cleanup ending", || cleanup.is_finished());

    release
        .send(())
        .expect("release the writer");
    writer
        .join()
        .expect("join the writer")
        .expect("the writer's create succeeds");
    cleanup.join().expect("join the cleanup");
}

/// A create that fails on a path someone else already holds releases its
/// registration, so the cleanup leaves that file alone.
#[test]
fn a_failed_create_leaves_the_file_at_its_path_alone() {
    static FILES: PendingTempFiles = PendingTempFiles::new(Duration::from_secs(60));
    let dir = tempfile::tempdir().expect("tempdir");
    let taken = dir.path().join("taken.tmp");
    fs::write(&taken, "another writer's").expect("stage the other writer's file");

    let Err(error) = FILES.track(&taken, || {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&taken)
    }) else {
        panic!("creating a file that exists must fail");
    };
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);

    FILES.remove_before_exit();
    assert!(
        taken.exists(),
        "the cleanup must not unlink a file the write never created: {taken:?}",
    );
}
