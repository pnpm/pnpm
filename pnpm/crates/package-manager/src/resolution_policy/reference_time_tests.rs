use super::PickPolicy;
use pnpm_config::Config;

#[test]
fn reference_time_sets_the_resolution_cutoff() {
    let now = pnpm_time::parse_rfc3339("2026-01-02T00:00:00Z").expect("valid");
    let config =
        Config { minimum_release_age: Some(24 * 60), reference_time: Some(now), ..Config::new() };
    let policy = PickPolicy::from_config(&config).expect("derive pick policy");
    assert_eq!(policy.published_by, Some(now - jiff::SignedDuration::from_hours(24)));
}
