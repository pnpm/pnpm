use super::{
    DepType,
    detect_dep_types,
};
use pnpm_lockfile::{
    Lockfile,
    PeerSatisfactionEdges,
    PkgNameVerPeer,
};
use pretty_assertions::assert_eq;
use std::{
    fmt::Write,
    path::Path,
};

const LENGTH: usize = 1_000;

/// A lockfile whose importer has `p0` as a devDependency and `q` as a
/// dependency. `p0` depends on `p1`, which depends on `p2`, and so on to
/// `p999`, and `q` depends on `p{shared}`.
fn lockfile(shared: usize) -> Lockfile {
    let mut packages = String::new();
    let mut snapshots = String::new();
    for index in 0..LENGTH {
        writeln!(packages, "  p{index}@1.0.0:\n    resolution: {{integrity: sha512-p{index}}}")
            .expect("write to a string");
        if index + 1 < LENGTH {
            writeln!(
                snapshots,
                "  p{index}@1.0.0:\n    dependencies:\n      p{}: 1.0.0",
                index + 1,
            )
        } else {
            writeln!(snapshots, "  p{index}@1.0.0: {{}}")
        }
        .expect("write to a string");
    }
    let text = format!(
        "lockfileVersion: '9.0'\n\
         importers:\n  .:\n    dependencies:\n      q:\n        specifier: 1.0.0\n        version: 1.0.0\n\
         \x20   devDependencies:\n      p0:\n        specifier: 1.0.0\n        version: 1.0.0\n\
         packages:\n  q@1.0.0:\n    resolution: {{integrity: sha512-q}}\n{packages}\
         snapshots:\n  q@1.0.0:\n    dependencies:\n      p{shared}: 1.0.0\n{snapshots}",
    );
    Lockfile::parse(&text, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile has content")
}

fn dep_type_of(dep_types: &super::DepTypes, name: &str) -> Option<DepType> {
    let key: PkgNameVerPeer = format!("{name}@1.0.0").parse().expect("parse the snapshot key");
    dep_types.get(&key).copied()
}

/// Every package of a chain a thousand snapshots deep is classified: those
/// above the package `q` shares are dev-only, and those from it down are
/// both.
#[test]
fn a_deep_chain_is_classified_to_its_end() {
    let shared = LENGTH / 2;
    let dep_types = detect_dep_types(&lockfile(shared), &PeerSatisfactionEdges::default());
    assert_eq!(dep_type_of(&dep_types, "q"), Some(DepType::ProdOnly));
    for index in 0..LENGTH {
        let expected = if index < shared { DepType::DevOnly } else { DepType::DevAndProd };
        assert_eq!(dep_type_of(&dep_types, &format!("p{index}")), Some(expected), "p{index}");
    }
}
