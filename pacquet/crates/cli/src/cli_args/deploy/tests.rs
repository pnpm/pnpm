use super::split_local_payload;

#[test]
fn split_local_payload_preserves_parentheses_in_path_before_peer_suffix() {
    assert_eq!(
        split_local_payload("../local(foo)/pkg(peer@1.0.0)"),
        ("../local(foo)/pkg", "(peer@1.0.0)"),
    );
}

#[test]
fn split_local_payload_preserves_parentheses_in_path_before_patch_suffix() {
    assert_eq!(
        split_local_payload("../local(foo)/pkg(patch_hash=abc)(peer@1.0.0)"),
        ("../local(foo)/pkg", "(patch_hash=abc)(peer@1.0.0)"),
    );
}
