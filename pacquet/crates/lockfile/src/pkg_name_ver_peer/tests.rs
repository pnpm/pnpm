use super::PkgNameVerPeer;
use pretty_assertions::assert_eq;

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
        let received = name_ver_peer.to_virtual_store_name();
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
