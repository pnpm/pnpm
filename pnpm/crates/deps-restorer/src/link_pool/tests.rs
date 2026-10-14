use super::{MACOS_LINK_THREADS, link_threads};

#[test]
fn link_threads_parses_a_positive_count() {
    assert_eq!(link_threads(Some("8")), 8);
    assert_eq!(link_threads(Some(" 2 ")), 2);
}

#[test]
fn link_threads_falls_back_to_the_default() {
    assert_eq!(link_threads(None), MACOS_LINK_THREADS);
    assert_eq!(link_threads(Some("")), MACOS_LINK_THREADS);
    assert_eq!(link_threads(Some("0")), MACOS_LINK_THREADS);
    assert_eq!(link_threads(Some("-1")), MACOS_LINK_THREADS);
    assert_eq!(link_threads(Some("four")), MACOS_LINK_THREADS);
}
