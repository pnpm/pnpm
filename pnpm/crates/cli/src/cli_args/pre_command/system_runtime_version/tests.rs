use super::{parse_bun_version, parse_deno_version};

#[test]
fn deno_version_is_read_from_the_leading_line() {
    let stdout =
        "deno 1.40.0 (release, x86_64-unknown-linux-gnu)\nv8 12.1.285.6\ntypescript 5.3.3\n";

    let version = parse_deno_version(stdout);

    assert_eq!(version.as_deref(), Some("1.40.0"));
}

#[test]
fn deno_prerelease_version_keeps_its_suffix() {
    let version = parse_deno_version("deno 2.0.0-rc.1 (release, aarch64-apple-darwin)\n");

    assert_eq!(version.as_deref(), Some("2.0.0-rc.1"));
}

#[test]
fn deno_version_is_none_when_the_output_has_no_version() {
    let outputs = ["denoland 1.40.0\n", "deno\n", "deno not-a-version\n", ""];

    for output in outputs {
        let version = parse_deno_version(output);
        dbg!(output, &version);
        assert!(version.is_none(), "unexpected version for {output:?}");
    }
}

#[test]
fn bun_version_is_the_whole_trimmed_output() {
    let version = parse_bun_version("1.1.0\n");

    assert_eq!(version.as_deref(), Some("1.1.0"));
}

#[test]
fn a_version_trailed_by_terminal_escapes_is_rejected() {
    let deno = parse_deno_version("deno 1.40.0\u{1b}[2J (release, x86_64-unknown-linux-gnu)\n");
    let bun = parse_bun_version("1.1.0\u{1b}[2J\n");

    dbg!(&deno, &bun);
    assert!(deno.is_none(), "unexpected Deno version");
    assert!(bun.is_none(), "unexpected Bun version");
}

#[test]
fn bun_version_is_none_when_the_output_is_not_a_version() {
    let outputs = ["bun 1.1.0\n", "1.1\n", ""];

    for output in outputs {
        let version = parse_bun_version(output);
        dbg!(output, &version);
        assert!(version.is_none(), "unexpected version for {output:?}");
    }
}
