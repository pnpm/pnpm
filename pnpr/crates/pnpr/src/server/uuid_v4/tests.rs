use super::{generate, hyphenate};

#[test]
fn hyphenate_groups_the_bytes_as_8_4_4_4_12() {
    let bytes = *b"0123456789abcdef";
    assert_eq!(hyphenate(&bytes), "30313233-3435-3637-3839-616263646566");
}

/// The version and variant bits are what make the value a v4 UUID rather
/// than 16 random bytes, and they sit inside two of the groups.
#[test]
fn generate_marks_the_version_and_variant() {
    for _ in 0..64 {
        let id = generate();
        let groups: Vec<&str> = id.split('-').collect();
        let lengths: Vec<usize> = groups
            .iter()
            .map(|group| group.len())
            .collect();
        assert_eq!(lengths, [8, 4, 4, 4, 12], "{id}");
        let hex_or_hyphen = |byte: u8| byte == b'-' || byte.is_ascii_hexdigit();
        assert!(id.bytes().all(hex_or_hyphen), "{id}");
        assert!(groups[2].starts_with('4'), "version nibble: {id}");
        assert!(groups[3].starts_with(['8', '9', 'a', 'b']), "variant bits: {id}");
    }
}
