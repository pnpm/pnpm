#![allow(
    clippy::needless_pass_by_value,
    reason = "nested test helpers take owned fixture values; by-value keeps call sites and assert ergonomics simple"
)]

use super::PkgNameVerPeer;
use pretty_assertions::assert_eq;

const DEFAULT_MAX_LENGTH: usize = 120;

fn name_peer_ver(name: &str, peer_ver: &str) -> PkgNameVerPeer {
    let peer_ver = peer_ver.to_string().parse().unwrap();
    PkgNameVerPeer::new(name.parse().unwrap(), peer_ver)
}

#[test]
fn parse() {
    fn case(input: &'static str, expected: PkgNameVerPeer) {
        eprintln!("CASE: {input:?}");
        let received: PkgNameVerPeer = input.parse().unwrap();
        assert_eq!(&received, &expected);
    }

    case(
        "react-json-view@1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)",
        name_peer_ver(
            "react-json-view",
            "1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)",
        ),
    );
    case("react-json-view@1.21.3", name_peer_ver("react-json-view", "1.21.3"));
    case(
        "@algolia/autocomplete-core@1.9.3(@algolia/client-search@4.18.0)(algoliasearch@4.18.0)(search-insights@2.6.0)",
        name_peer_ver(
            "@algolia/autocomplete-core",
            "1.9.3(@algolia/client-search@4.18.0)(algoliasearch@4.18.0)(search-insights@2.6.0)",
        ),
    );
    case("@algolia/autocomplete-core@1.9.3", name_peer_ver("@algolia/autocomplete-core", "1.9.3"));
}

/// Mirrors pnpm's `parse()` test, `file:` leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L61-L66)).
#[test]
fn parse_local_tarball_file_protocol() {
    let key: PkgNameVerPeer =
        "tar-pkg@file:../tar-pkg-1.0.0.tgz".parse().expect("parse file: tarball key");
    assert_eq!(key.to_string(), "tar-pkg@file:../tar-pkg-1.0.0.tgz");
}

/// Mirrors pnpm's `parse()` test, patch-hash + peer leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L68-L73)).
/// `PkgVerPeer` lumps `(patch_hash=...)(<peers>)` into the `peer`
/// field — the raw key still round-trips.
#[test]
fn parse_patch_hash_and_peer_suffix_round_trip() {
    let raw = "foo@1.0.0(patch_hash=0000)(@types/babel__core@7.1.14)";
    let key: PkgNameVerPeer = raw.parse().expect("parse patch-hash + peer-variant key");
    assert_eq!(key.to_string(), raw);
}

/// Mirrors pnpm's `parse()` test, scope-with-parens leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L54-L59)).
#[test]
fn parse_scope_with_parens_round_trip() {
    let raw = "@(-.-)/foo@1.0.0(@types/babel__core@7.1.14)(foo@1.0.0)";
    let key: PkgNameVerPeer = raw.parse().expect("parse scope-with-parens key");
    assert_eq!(key.to_string(), raw);
    assert_eq!(
        key.without_peer().to_string(),
        "@(-.-)/foo@1.0.0",
        "without_peer must preserve the parens inside the scope",
    );
}

#[test]
fn to_virtual_store_name() {
    fn case(input: &'static str, expected: &'static str) {
        eprintln!("CASE: {input:?}");
        let name_ver_peer: PkgNameVerPeer = input.parse().unwrap();
        dbg!(&name_ver_peer);
        let received = name_ver_peer.to_virtual_store_name(DEFAULT_MAX_LENGTH);
        assert_eq!(received, expected);
    }

    case("ts-node@10.9.1", "ts-node@10.9.1");
    case(
        "ts-node@10.9.1(@types/node@18.7.19)(typescript@5.1.6)",
        "ts-node@10.9.1_@types+node@18.7.19_typescript@5.1.6",
    );
    case(
        "@babel/plugin-proposal-object-rest-spread@7.12.1",
        "@babel+plugin-proposal-object-rest-spread@7.12.1",
    );
    case(
        "@babel/plugin-proposal-object-rest-spread@7.12.1(@babel/core@7.12.9)",
        "@babel+plugin-proposal-object-rest-spread@7.12.1_@babel+core@7.12.9",
    );

    // `file:` resolution emitted by an injected workspace package.
    // Without escaping `:`, the resulting directory name is invalid
    // on NTFS / FAT (`ERROR_INVALID_NAME (123)`). Mirrors upstream's
    // [`depPathToFilename` regex](https://github.com/pnpm/pnpm/blob/1819226b51/deps/path/src/index.ts#L170)
    // which folds `[\\/:*?"<>|#]` into `+`.
    case(
        "project-1@file:project-1(is-positive@1.0.0)",
        "project-1@file+project-1_is-positive@1.0.0",
    );
}

#[test]
fn without_peer_strips_peer_suffix() {
    let key: PkgNameVerPeer =
        "react-dom@17.0.2(react@17.0.2)".parse().expect("parse react-dom peer-variant key");
    let bare = key.without_peer();
    assert_eq!(bare.to_string(), "react-dom@17.0.2");
}

/// Mirrors pnpm's `removeSuffix` test
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L151-L153)).
/// Pacquet's `PkgVerPeer` lumps the patch-hash and peer segments into
/// one `peer` field, so `without_peer` strips both. The `packages:`
/// key shape diverges from `getPkgIdWithPatchHash` here — separate
/// parity gap, separate PR.
#[test]
fn without_peer_strips_patch_hash_alongside_peer_suffix() {
    let key: PkgNameVerPeer = "foo@1.0.0(patch_hash=0000)(@types/babel__core@7.1.14)"
        .parse()
        .expect("parse patch-hash plus peer-variant key");
    let bare = key.without_peer();
    assert_eq!(bare.to_string(), "foo@1.0.0");
}

/// Mirrors pnpm's `getPkgIdWithPatchHash` test, scoped-name leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L140-L143)).
#[test]
fn without_peer_strips_peer_from_scoped_name() {
    let key: PkgNameVerPeer = "@foo/bar@1.0.0(patch_hash=zzzz)(@types/node@18.0.0)"
        .parse()
        .expect("parse scoped patch-hash + peer-variant key");
    let bare = key.without_peer();
    assert_eq!(bare.to_string(), "@foo/bar@1.0.0");
}

/// Mirrors pnpm's `tryGetPackageId` test, nested-peer leg
/// ([`deps/path/test/index.ts`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/test/index.ts#L112)).
#[test]
fn without_peer_strips_peer_with_nested_parens() {
    let key: PkgNameVerPeer = "foo@1.0.0(@types/babel__core@7.1.14(is-odd@1.0.0))"
        .parse()
        .expect("parse nested-peer key");
    let bare = key.without_peer();
    assert_eq!(bare.to_string(), "foo@1.0.0");
}

/// Runtime entries keep their `runtime:` prefix on the metadata-map
/// key — pnpm's `packages:` entry carries the same prefix, so
/// dropping it would unhook the snapshot from its metadata.
#[test]
fn without_peer_preserves_runtime_prefix() {
    let key: PkgNameVerPeer =
        "node@runtime:22.0.0(react@17.0.2)".parse().expect("parse runtime peer-variant key");
    let bare = key.without_peer();
    assert_eq!(bare.to_string(), "node@runtime:22.0.0");
}

/// Regression for [#11939](https://github.com/pnpm/pnpm/issues/11939):
/// the babylon shape `link:<rel-path>(<peers>)` parses into a
/// `PkgNameVerPeer` whose `version()` retains an unbalanced `)`. Peer
/// stripping is structural and must not re-parse.
#[test]
fn without_peer_handles_workspace_link_with_peer_suffix() {
    let key: PkgNameVerPeer = "link:../../../dev/sharedUiComponents(\
        @fluentui/react-components@9.73.8)\
        (react-dom@18.3.1)\
        (react@18.3.1)"
        .parse()
        .expect("parse workspace link with peer suffix");
    let bare = key.without_peer();
    let rendered = bare.to_string();
    assert!(
        rendered.starts_with("link:../../../dev/sharedUiComponents("),
        "name half of the key must survive verbatim; got {rendered:?}",
    );
    assert!(!rendered.contains(")("), "peer suffix must be stripped; got {rendered:?}");
}

/// The user-reported macOS errno-63 case: a vitest snapshot key whose
/// escaped filename blows past 120 bytes. The shortening must produce a
/// name that fits inside `max_length` so `fs::create_dir_all` doesn't
/// hit `ENAMETOOLONG`.
#[test]
fn to_virtual_store_name_shortens_user_reported_vitest_case() {
    let input: PkgNameVerPeer = "vitest@4.1.6\
        (@opentelemetry/api@1.9.1)\
        (@types/node@24.12.4)\
        (@vitest/browser-playwright@4.1.6)\
        (@vitest/coverage-v8@4.1.6)\
        (happy-dom@20.9.0)\
        (jsdom@26.1.0)\
        (canvas@3.2.1)\
        (msw@2.12.14)\
        (yaml@2.8.4)"
        .parse()
        .unwrap();
    let received = input.to_virtual_store_name(DEFAULT_MAX_LENGTH);
    assert_eq!(received.len(), DEFAULT_MAX_LENGTH);
    let (_, hash) = received.rsplit_once('_').expect("hash suffix");
    assert_eq!(hash.len(), 32);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}
