use super::{CombinationSearch, MAX_COMBINATIONS, PeerGroups, within_combination_limit};

#[test]
fn expanded_candidates_stop_at_the_search_budget() {
    let candidates: Vec<_> = (0..12)
        .map(|index| {
            vec![
                format!("provider-{index}@1.0.0(nested@1.0.0)").parse().unwrap(),
                format!("provider-{index}@1.0.0").parse().unwrap(),
            ]
        })
        .collect();
    assert!(!within_combination_limit(&candidates));
    let mut search = CombinationSearch::new(PeerGroups::Full, "unmatched");
    assert_eq!(search.remaining, MAX_COMBINATIONS);
    assert_eq!(search.find(&candidates), None);
    assert_eq!(search.remaining, 0);
    assert_eq!(search.find(&candidates), None);
}
