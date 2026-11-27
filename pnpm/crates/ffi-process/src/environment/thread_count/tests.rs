use super::thread_count;

#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn reads_the_thread_count_past_a_command_name_with_parentheses() {
    use super::parse_stat_thread_count;
    let fields_3_to_19 = "S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16";
    let stat = format!("42 (a) b (c)) {fields_3_to_19} 7 0 1234\n");
    assert_eq!(parse_stat_thread_count(&stat), Some(7));
    assert_eq!(parse_stat_thread_count("42 (pnpm) S 1"), None);
    assert_eq!(parse_stat_thread_count("no name"), None);
}

/// The test harness runs this on a thread of its own, so the count is
/// never below two, and it grows by the thread spawned here while that
/// thread waits.
#[test]
fn counts_a_spawned_thread() {
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
