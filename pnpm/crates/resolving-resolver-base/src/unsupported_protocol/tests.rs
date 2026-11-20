use super::UnsupportedProtocolError;

#[test]
fn detects_a_protocol_prefix() {
    let err = UnsupportedProtocolError::detect("patch:got@npm%3A11.8.2#~/.yarn/patches/got.patch")
        .expect("detected");
    assert_eq!(err.protocol, "patch:");
    assert_eq!(err.specifier, "patch:got@npm%3A11.8.2#~/.yarn/patches/got.patch");
}

#[test]
fn ignores_paths_and_drive_letters() {
    for specifier in ["../pkg", "./a:b/c", "dir/sub:x", "C:/pkg", "C:pkg", "~/pkg", "pkg.tgz"] {
        assert!(UnsupportedProtocolError::detect(specifier).is_none(), "{specifier}");
    }
}
