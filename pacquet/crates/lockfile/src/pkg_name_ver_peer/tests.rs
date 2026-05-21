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
