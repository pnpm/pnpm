use super::{generate, hyphenate};

#[test]
fn hyphenate_groups_the_bytes_as_8_4_4_4_12() {
    let bytes = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff,
    ];
    assert_eq!(hyphenate(&bytes), "00112233-4455-6677-8899-aabbccddeeff");
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
        assert_eq!(&groups[2][..1], "4", "version nibble: {id}");
        assert!(matches!(&groups[3][..1], "8" | "9" | "a" | "b"), "variant bits: {id}");
    }
}
