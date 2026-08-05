use super::script_thread_count;

#[test]
fn script_threads_are_bounded_by_work_and_the_safety_cap() {
    assert_eq!(script_thread_count(0, 0), 1);
    assert_eq!(script_thread_count(8, 3), 3);
    assert_eq!(script_thread_count(u32::MAX, usize::MAX), 256);
}
