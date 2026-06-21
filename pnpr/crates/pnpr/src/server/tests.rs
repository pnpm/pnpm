use super::token_timestamp_millis;

#[test]
fn token_timestamp_millis_saturates_before_i64_conversion() {
    assert_eq!(token_timestamp_millis(42), 42_000);
    assert_eq!(token_timestamp_millis(u64::MAX), i64::MAX / 1000 * 1000);
}
