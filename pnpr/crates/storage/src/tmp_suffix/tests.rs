use super::{MAX_TMP_SUFFIX_BYTES, tmp_suffix};

#[test]
fn the_longest_suffix_is_the_bound() {
    assert_eq!(tmp_suffix(u32::MAX, u64::MAX, u64::MAX).len(), MAX_TMP_SUFFIX_BYTES);
}

#[test]
fn a_short_suffix_keeps_the_random_part_full_width() {
    assert_eq!(tmp_suffix(1, 0, 1), ".tmp.1.0.0000000000000001");
}
