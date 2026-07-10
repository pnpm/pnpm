use super::{ProjectSnapshot, ResolvedDependencySpec};
use pacquet_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;
use text_block_macros::text_block;

const YAML: &str = text_block! {
    "dependencies:"
    "  react:"
    "    specifier: ^17.0.2"
    "    version: 17.0.2"
    "  react-dom:"
    "    specifier: ^17.0.2"
    "    version: 17.0.2(react@17.0.2)"
    "optionalDependencies:"
    "  '@types/node':"
    "    specifier: ^18.7.19"
    "    version: 18.7.19"
    "devDependencies:"
    "  ts-node:"
    "    specifier: 10.9.1"
    "    version: 10.9.1(@types/node@18.7.19)(typescript@5.1.6)"
    "  typescript:"
    "    specifier: ^5.1.6"
    "    version: 5.1.6"
};

fn fixture_project_snapshot() -> ProjectSnapshot {
    serde_saphyr::from_str(YAML).unwrap()
}

#[test]
fn dependencies_by_groups() {
    use DependencyGroup::{Dev, Optional, Peer, Prod};

    macro_rules! case {
        ($input:expr => $output:expr) => {{
            let groups = $input;
            eprintln!("CASE: {groups:?}");
            let mut received: Vec<_> = fixture_project_snapshot()
                .dependencies_by_groups(groups)
                .map(|(name, ResolvedDependencySpec { specifier, version })| {
                    (name.to_string(), specifier.to_string(), version.to_string())
                })
                .collect();
            received.sort(); // TODO: remove this line after switching to IndexMap
            let expected = $output.map(|(name, specifier, version): (&str, &str, &str)| {
                (name.to_string(), specifier.to_string(), version.to_string())
            });
            assert_eq!(received, expected);
        }};
    }

    case!([] => []);
    case!([Prod] => [
        ("react", "^17.0.2", "17.0.2"),
        ("react-dom", "^17.0.2", "17.0.2(react@17.0.2)"),
    ]);
    case!([Peer] => []);
    case!([Optional] => [
        ("@types/node", "^18.7.19", "18.7.19"),
    ]);
    case!([Dev] => [
        ("ts-node", "10.9.1", "10.9.1(@types/node@18.7.19)(typescript@5.1.6)"),
        ("typescript", "^5.1.6", "5.1.6"),
    ]);
    case!([Prod, Peer] => [
        ("react", "^17.0.2", "17.0.2"),
        ("react-dom", "^17.0.2", "17.0.2(react@17.0.2)"),
    ]);
    case!([Prod, Peer, Optional] => [
        ("@types/node", "^18.7.19", "18.7.19"),
        ("react", "^17.0.2", "17.0.2"),
        ("react-dom", "^17.0.2", "17.0.2(react@17.0.2)"),
    ]);
    case!([Prod, Peer, Optional, Dev] => [
        ("@types/node", "^18.7.19", "18.7.19"),
        ("react", "^17.0.2", "17.0.2"),
        ("react-dom", "^17.0.2", "17.0.2(react@17.0.2)"),
        ("ts-node", "10.9.1", "10.9.1(@types/node@18.7.19)(typescript@5.1.6)"),
        ("typescript", "^5.1.6", "5.1.6"),
    ]);
}
