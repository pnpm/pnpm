use super::{CombinationSearch, MAX_COMBINATIONS, PeerGroups};

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
    let mut search = CombinationSearch::new(PeerGroups::Full, "unmatched");
    assert_eq!(search.remaining, MAX_COMBINATIONS);
    assert_eq!(search.find(&candidates), None);
    assert_eq!(search.remaining, 0);
    assert_eq!(search.find(&candidates), None);
}

#[test]
fn complete_provider_set_is_checked_before_omissions_exhaust_the_budget() {
    let names: Vec<_> = (0..13)
        .map(|index| format!("provider-{index:02}@1.0.0"))
        .collect();
    let candidates: Vec<_> = names
        .iter()
        .map(|name| vec![name.parse().unwrap()])
        .collect();
    let hash = pnpm_crypto_hash::create_short_hash(&names.join(")("));
    let recovered = super::match_candidates(&candidates, &hash).unwrap();
    assert_eq!(recovered.len(), names.len());
    for name in names {
        assert!(recovered.contains(&name.parse().unwrap()));
    }
}

#[test]
fn large_peer_sets_do_not_use_the_call_stack() {
    let candidates: Vec<_> = (0..10_000)
        .map(|index| vec![format!("provider-{index}@1.0.0").parse().unwrap()])
        .collect();
    let mut search = CombinationSearch::new(PeerGroups::Full, "unmatched");
    search.remaining = 2;
    assert_eq!(search.find(&candidates), None);
    assert_eq!(search.remaining, 0);
}
