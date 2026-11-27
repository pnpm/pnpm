#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn reads_the_thread_count_past_any_command_name() {
    use super::parse_stat_thread_count;
    let fields_3_to_19 = "S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16";
    let stat = format!("42 (a) b (c)) {fields_3_to_19} 7 0 1234\n");
    assert_eq!(parse_stat_thread_count(stat.as_bytes()), Some(7));
    let mut non_utf8 = b"42 (\xff\xfe) ".to_vec();
    non_utf8.extend_from_slice(format!("{fields_3_to_19} 3 0\n").as_bytes());
    assert_eq!(parse_stat_thread_count(&non_utf8), Some(3));
    assert_eq!(parse_stat_thread_count(b"42 (pnpm) S 1"), None);
    assert_eq!(parse_stat_thread_count(b"no name"), None);
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn a_zero_thread_count_is_no_count() {
    let fields_3_to_19 = "S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16";
    let stat = format!("42 (pnpm) {fields_3_to_19} 0 0\n");
    assert_eq!(super::parse_stat_thread_count(stat.as_bytes()), None);
}

/// The test harness runs this on a thread of its own, so the count is
/// never below two, and it grows by the thread spawned here while that
/// thread waits.
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "netbsd",
))]
#[test]
fn counts_a_spawned_thread() {
    use super::thread_count;
    let before = thread_count().expect("count the threads");
    assert!(before >= 2, "the harness thread and this one, got {before}");
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let waiter = std::thread::spawn(move || wait.recv());
    let during = thread_count().expect("count the threads");
    release
        .send(())
        .expect("release the waiting thread");
    waiter
        .join()
        .expect("join the waiting thread")
        .expect("receive the release");
    assert!(during >= 3, "the spawned thread is counted, got {during}");
}
