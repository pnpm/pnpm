use super::publish_date_policy_key;

#[test]
fn trusted_version_lists_cannot_collide_on_embedded_delimiters() {
    let cutoff = "2026-01-01T00:00:00Z".parse().unwrap();
    let combined = ["a\0b".to_string()];
    let separate = ["a".to_string(), "b".to_string()];
    assert_ne!(
        publish_date_policy_key(cutoff, Some(&combined)),
        publish_date_policy_key(cutoff, Some(&separate)),
    );
}
