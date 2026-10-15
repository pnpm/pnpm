use super::rustup_default_windows_host;
use pretty_assertions::assert_eq;

#[test]
fn rustup_default_host_chooses_the_whole_triple() {
    for triple in ["x86_64-pc-windows-gnu", "aarch64-pc-windows-gnullvm", "i686-pc-windows-msvc"] {
        assert_eq!(rustup_default_windows_host(Some(triple.to_string())).as_deref(), Some(triple));
    }
}

#[test]
fn an_unsupported_rustup_default_host_is_ignored() {
    for triple in
        ["x86_64-unknown-linux-gnu", "thumbv7a-pc-windows-msvc", "x86_64-pc-windows-other"]
    {
        assert_eq!(rustup_default_windows_host(Some(triple.to_string())), None);
    }
    assert_eq!(rustup_default_windows_host(None), None);
}
