use super::{MAX_DEMOTE_READS, SentinelPlaceholders};

#[test]
fn replace_minimal_returns_accepted_rendering_when_demotion_budget_expires() {
    let entries: Vec<String> = (0..300)
        .map(|index| format!("${{V{index}}}"))
        .collect();
    let text = entries.join(",");
    let sentinels = SentinelPlaceholders::new(&text).unwrap();
    let mut probe_calls = 0u32;
    let replaced = sentinels
        .replace_minimal(&text, |candidate| {
            probe_calls += 1;
            !candidate.contains("${")
        })
        .unwrap();
    assert_eq!(probe_calls, MAX_DEMOTE_READS);
    assert!(!replaced.contains("${"));
    assert_eq!(sentinels.restore(&text, &replaced), text);
}
