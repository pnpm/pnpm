use super::tmp_suffix;

#[test]
fn a_short_suffix_keeps_the_random_part_full_width() {
    assert_eq!(tmp_suffix(1, 0, 1), ".tmp.1.0.0000000000000001");
}
