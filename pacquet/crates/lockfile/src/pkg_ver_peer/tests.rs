use super::{ParsePkgVerPeerError, PkgVerPeer, Prefix};
use node_semver::Version;
use pretty_assertions::assert_eq;

fn assert_ver_peer<Ver, Peer>(received: PkgVerPeer, expected_version: Ver, expected_peer: Peer)
where
    Ver: Into<Version>,
    Peer: Into<String>,
{
    dbg!(&received);
    let expected_version = expected_version.into();
    let expected_peer = expected_peer.into();
    assert_eq!((received.version(), received.peer()), (&expected_version, expected_peer.as_str()));
    assert_eq!(received.into_tuple(), (expected_version, expected_peer));
}

fn decode_encode_case<Decode, Encode>(input: &str, decode: Decode, encode: Encode)
where
    Decode: Fn(&str) -> PkgVerPeer,
    Encode: Fn(&PkgVerPeer) -> String,
{
    eprintln!("CASE: {input:?}");
    let peer_ver = decode(input);
    dbg!(&peer_ver);
    let output = encode(&peer_ver);
    assert_eq!(input, output);
}

#[test]
fn parse_ok() {
    fn case<Ver, Peer>(input: &'static str, (expected_version, expected_peer): (Ver, Peer))
    where
        Ver: Into<Version>,
        Peer: Into<String>,
    {
        eprintln!("CASE: {input:?}");
        assert_ver_peer(input.parse().unwrap(), expected_version, expected_peer);
    }

    case(
        "1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)",
        ((1, 21, 3), "(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)"),
    );
    case("1.21.3(react@17.0.2)", ((1, 21, 3), "(react@17.0.2)"));
    case(
        "1.21.3-rc.0(react@17.0.2)",
        ("1.21.3-rc.0".parse::<Version>().unwrap(), "(react@17.0.2)"),
    );
    case("1.21.3", ((1, 21, 3), ""));
    case("1.21.3-rc.0", ("1.21.3-rc.0".parse::<Version>().unwrap(), ""));
}

#[test]
fn parse_err() {
    macro_rules! case {
        ($input:expr => $message:expr, $variant:pat) => {{
            let input = $input;
            eprintln!("CASE: {input:?}");
            let error = input.parse::<PkgVerPeer>().unwrap_err();
            dbg!(&error);
            assert_eq!(error.to_string(), $message);
            assert!(matches!(error, $variant));
        }};
    }
    case!("1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2" => "Mismatch parenthesis", ParsePkgVerPeerError::MismatchParenthesis);
    case!("1.21.3(" => "Mismatch parenthesis", ParsePkgVerPeerError::MismatchParenthesis);
    case!("1.21.3)" => "Mismatch parenthesis", ParsePkgVerPeerError::MismatchParenthesis);
    case!("a.b.c" => "Failed to parse the version part: Failed to parse version.", ParsePkgVerPeerError::ParseVersionFailure(_));
}

#[test]
fn deserialize_ok() {
    fn case<Ver, Peer>(input: &'static str, (expected_version, expected_peer): (Ver, Peer))
    where
        Ver: Into<Version>,
        Peer: Into<String>,
    {
        eprintln!("CASE: {input:?}");
        assert_ver_peer(serde_saphyr::from_str(input).unwrap(), expected_version, expected_peer);
    }

    case(
        "1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)",
        ((1, 21, 3), "(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)"),
    );
    case("1.21.3(react@17.0.2)", ((1, 21, 3), "(react@17.0.2)"));
    case(
        "1.21.3-rc.0(react@17.0.2)",
        ("1.21.3-rc.0".parse::<Version>().unwrap(), "(react@17.0.2)"),
    );
    case("1.21.3", ((1, 21, 3), ""));
    case("1.21.3-rc.0", ("1.21.3-rc.0".parse::<Version>().unwrap(), ""));
}

#[test]
fn parse_to_string() {
    let case =
        |input| decode_encode_case(input, |input| input.parse().unwrap(), ToString::to_string);
    case("1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)");
    case("1.21.3(react@17.0.2)");
    case("1.21.3-rc.0(react@17.0.2)");
    case("1.21.3");
    case("1.21.3-rc.0");
}

#[test]
fn deserialize_serialize() {
    let case = |input| {
        decode_encode_case(
            input,
            |input| serde_saphyr::from_str(input).unwrap(),
            |ver_peer| serde_saphyr::to_string(&ver_peer).unwrap().trim().to_string(),
        )
    };
    case("1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)");
    case("1.21.3(react@17.0.2)");
    case("1.21.3-rc.0(react@17.0.2)");
    case("1.21.3");
    case("1.21.3-rc.0");
}

/// Pnpm v11 writes runtime depPaths with a `runtime:` prefix in
/// front of the semver (e.g. `node@runtime:22.0.0` in the
/// lockfile's `packages:` / `snapshots:` keys). Pacquet's parser
/// must accept the prefix and preserve it through Display so a
/// `serde_saphyr` round-trip stays byte-stable. Pre-runtime
/// callers that only read `version()` continue to see the bare
/// semver — the prefix is exposed separately via
/// [`PkgVerPeer::prefix`].
#[test]
fn parse_runtime_prefix_round_trips() {
    let parsed: PkgVerPeer = "runtime:22.0.0".parse().expect("parse runtime version");
    dbg!(&parsed);
    assert_eq!(parsed.prefix(), Prefix::Runtime);
    assert_eq!(parsed.version(), &"22.0.0".parse::<Version>().unwrap());
    assert_eq!(parsed.peer(), "");
    assert_eq!(parsed.to_string(), "runtime:22.0.0");
}

/// The `runtime:` prefix composes with the peer-dependency suffix.
/// In real lockfiles a runtime entry never carries a peer (pnpm
/// doesn't write one), but pacquet's parser doesn't need to make
/// that assumption — pin the combinatorial behavior here so a
/// future runtime-with-peer fixture parses cleanly rather than
/// erroring.
#[test]
fn parse_runtime_prefix_with_peer_suffix() {
    let parsed: PkgVerPeer =
        "runtime:22.0.0(node@22.0.0)".parse().expect("parse runtime with peer");
    dbg!(&parsed);
    assert_eq!(parsed.prefix(), Prefix::Runtime);
    assert_eq!(parsed.version(), &"22.0.0".parse::<Version>().unwrap());
    assert_eq!(parsed.peer(), "(node@22.0.0)");
    assert_eq!(parsed.to_string(), "runtime:22.0.0(node@22.0.0)");
}

/// Bare semver (the only shape pre-pnpm-v11) has
/// `Prefix::None` and Display omits the prefix.
#[test]
fn parse_bare_semver_has_no_prefix() {
    let parsed: PkgVerPeer = "1.21.3".parse().expect("parse bare");
    assert_eq!(parsed.prefix(), Prefix::None);
    assert_eq!(parsed.to_string(), "1.21.3");
}

/// A version string that *looks* like a prefix mid-name (i.e.
/// `1.21.3-runtime:`) must NOT be parsed as a `runtime:` prefix —
/// the prefix detection is anchored at the start. This pins the
/// `strip_prefix` choice (vs. e.g. `contains`) so a regression
/// can't accidentally widen the match.
#[test]
fn parse_runtime_substring_in_version_is_not_a_prefix() {
    // `1.21.3-runtime` is a valid semver pre-release tag.
    let parsed: PkgVerPeer = "1.21.3-runtime".parse().expect("parse semver pre-release");
    assert_eq!(parsed.prefix(), Prefix::None);
    assert_eq!(parsed.version(), &"1.21.3-runtime".parse::<Version>().unwrap());
}

/// Serde round-trip on a runtime version — pacquet stores
/// `PkgVerPeer` inside `ResolvedDependencySpec`, `PkgNameVerPeer`,
/// and others; their serde proxies through `PkgVerPeer`'s
/// `try_from = "Cow<str>"` / `into = "String"` impls, so this
/// test pins the wire shape end-to-end.
#[test]
fn serde_round_trip_runtime_prefix() {
    let parsed: PkgVerPeer = serde_saphyr::from_str("runtime:22.0.0").expect("deserialize runtime");
    assert_eq!(parsed.prefix(), Prefix::Runtime);
    assert_eq!(parsed.version(), &"22.0.0".parse::<Version>().unwrap());
    let serialized = serde_saphyr::to_string(&parsed).expect("serialize").trim().to_string();
    assert_eq!(serialized, "runtime:22.0.0");
}
