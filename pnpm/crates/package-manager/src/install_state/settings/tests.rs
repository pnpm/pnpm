use super::reference_time_moved_back;

#[test]
fn reference_time_drifts_only_when_it_moves_back() {
    let at = |value: &str| pnpm_time::parse_rfc3339(value).expect("valid");
    let now = at("2026-03-01T00:00:00Z");
    let early = Some("2026-01-01T00:00:00Z");
    let late = Some("2026-02-01T00:00:00Z");
    let ahead = Some("2026-03-01T00:05:00Z");
    assert!(!reference_time_moved_back(early, late, now), "moved forward");
    assert!(!reference_time_moved_back(early, early, now), "unchanged");
    assert!(!reference_time_moved_back(early, None, now), "removed while in the past");
    assert!(!reference_time_moved_back(None, None, now), "never set");
    assert!(reference_time_moved_back(late, early, now), "moved back");
    assert!(reference_time_moved_back(None, early, now), "set where the record used its own clock");
    assert!(reference_time_moved_back(ahead, None, now), "removed while ahead of the clock");
}
