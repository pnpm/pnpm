use super::{ParsePkgNameVerError, PkgNameVer};
use node_semver::Version;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;

fn name_ver(name: &str, ver: impl Into<Version>) -> PkgNameVer {
    PkgNameVer::new(name.parse().unwrap(), ver.into())
}

#[test]
fn parse_ok() {
    fn case(input: &'static str, expected: PkgNameVer) {
        eprintln!("CASE: {input:?}");
        let received: PkgNameVer = input.parse().unwrap();
        assert_eq!(&received, &expected);
    }

    case("ts-node@10.9.1", name_ver("ts-node", (10, 9, 1)));
    case("@types/node@18.7.19", name_ver("@types/node", (18, 7, 19)));
    case("typescript@5.1.6", name_ver("typescript", (5, 1, 6)));
    case("foo@0.1.2-alpha.0", name_ver("foo", Version::parse("0.1.2-alpha.0").unwrap()));
    case("@foo/bar@0.1.2-rc.0", name_ver("@foo/bar", Version::parse("0.1.2-rc.0").unwrap()));
}

#[test]
fn deserialize_ok() {
    fn case(input: &'static str, expected: PkgNameVer) {
        eprintln!("CASE: {input:?}");
        let received: PkgNameVer = serde_saphyr::from_str(input).unwrap();
        assert_eq!(&received, &expected);
    }

    case("ts-node@10.9.1", name_ver("ts-node", (10, 9, 1)));
    case("'@types/node@18.7.19'", name_ver("@types/node", (18, 7, 19)));
    case("typescript@5.1.6", name_ver("typescript", (5, 1, 6)));
    case("foo@0.1.2-alpha.0", name_ver("foo", Version::parse("0.1.2-alpha.0").unwrap()));
    case("'@foo/bar@0.1.2-rc.0'", name_ver("@foo/bar", Version::parse("0.1.2-rc.0").unwrap()));
}

#[test]
fn parse_err() {
    macro_rules! case {
        ($title:literal: $input:expr => $message:expr, $pattern:pat) => {{
            let title = $title;
            let input = $input;
            eprintln!("CASE: {title} (input = {input:?})");
            let error = input.parse::<PkgNameVer>().unwrap_err();
            dbg!(&error);
            assert_eq!(error.to_string(), $message);
            assert!(matches!(&error, $pattern));
        }};
    }

    case!("Empty input": "" => "Input is empty", ParsePkgNameVerError::EmptyInput);
    case!("Non-scope name without version": "ts-node" => "Suffix is missing", ParsePkgNameVerError::MissingSuffix);
    case!("Scoped name without version": "@types/node" => "Suffix is missing", ParsePkgNameVerError::MissingSuffix);
    case!("Non-scope name with empty version": "ts-node" => "Suffix is missing", ParsePkgNameVerError::MissingSuffix);
    case!("Scoped name with empty version": "@types/node" => "Suffix is missing", ParsePkgNameVerError::MissingSuffix);
    case!("Missing name": "10.9.1" => "Suffix is missing", ParsePkgNameVerError::MissingSuffix); // can't fix without parser combinator
    case!("Empty non-scope name": "@19.9.1" => "Suffix is missing", ParsePkgNameVerError::MissingSuffix); // can't fix without parser combinator
    case!("Empty scoped name": "@@18.7.19" => "Name is empty", ParsePkgNameVerError::EmptyName);
}

#[test]
fn to_string() {
    let string = name_ver("ts-node", (10, 9, 1)).to_string();
    assert_eq!(string, "ts-node@10.9.1");
}

#[test]
fn serialize() {
    let received = name_ver("ts-node", (10, 9, 1)).pipe_ref(serde_saphyr::to_string).unwrap();
    let expected = "ts-node@10.9.1\n";
    assert_eq!(received, expected);
}
