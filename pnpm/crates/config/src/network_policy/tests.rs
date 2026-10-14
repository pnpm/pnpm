use super::NetworkPolicy;
use pretty_assertions::assert_eq;

#[test]
fn offline_decides_alone() {
    assert_eq!(NetworkPolicy::from_settings(true, false), NetworkPolicy::Offline);
    assert_eq!(NetworkPolicy::from_settings(true, true), NetworkPolicy::Offline);
}

#[test]
fn prefer_offline_decides_while_the_registry_is_reachable() {
    assert_eq!(NetworkPolicy::from_settings(false, true), NetworkPolicy::PreferCache);
    assert_eq!(NetworkPolicy::from_settings(false, false), NetworkPolicy::Online);
}

#[test]
fn each_state_answers_the_three_questions_differently() {
    let answers = |policy: NetworkPolicy| {
        (policy.forbids_registry(), policy.reads_cache_first(), policy.prefers_reachable_cache())
    };
    assert_eq!(answers(NetworkPolicy::Online), (false, false, false));
    assert_eq!(answers(NetworkPolicy::PreferCache), (false, true, true));
    assert_eq!(answers(NetworkPolicy::Offline), (true, true, false));
}
