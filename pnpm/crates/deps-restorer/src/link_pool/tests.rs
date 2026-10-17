use super::{MACOS_LINK_THREADS, link_threads};

#[test]
fn link_threads_parses_a_positive_count() {
    assert_eq!(link_threads(Some("8")), Some(8));
    assert_eq!(link_threads(Some(" 2 ")), Some(2));
}

#[test]
fn link_threads_turns_the_pool_off_for_zero() {
    assert_eq!(link_threads(Some("0")), None);
}

#[test]
fn link_threads_falls_back_to_the_default() {
    assert_eq!(link_threads(None), Some(MACOS_LINK_THREADS));
    assert_eq!(link_threads(Some("")), Some(MACOS_LINK_THREADS));
    assert_eq!(link_threads(Some("-1")), Some(MACOS_LINK_THREADS));
    assert_eq!(link_threads(Some("four")), Some(MACOS_LINK_THREADS));
}
