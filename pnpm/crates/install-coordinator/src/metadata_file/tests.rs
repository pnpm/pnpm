use super::MetadataFile;
use std::{
    ffi::CString,
    os::unix::ffi::OsStrExt as _,
    sync::mpsc,
    thread,
    time::Duration,
};

/// Opening a FIFO read-only waits for a writer, and a metadata path never
/// gets one, so capture has to reject it on its type instead of waiting.
#[test]
fn capturing_a_fifo_rejects_it_instead_of_waiting_for_a_writer() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("package.json");
    let name = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: the name is NUL-terminated and outlives the call.
    let created = unsafe { libc::mkfifo(name.as_ptr(), 0o644) };
    assert_eq!(created, 0, "mkfifo: {}", std::io::Error::last_os_error());

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || sender.send(MetadataFile::capture(path).map(|_| ())));
    let captured = receiver
        .recv_timeout(Duration::from_secs(30))
        .expect("capture returns instead of waiting for a writer");

    let report = captured.expect_err("a FIFO is not a regular file");
    let rendered = format!("{report:?}");
    assert!(rendered.contains("not a regular file or symlink"), "{rendered}");
}
