use super::MemoryTree;
use crate::{
    dependencies::Reach,
    graph::{CrateGraph, DependencyKind, Edge},
    leaks::{self, Leak},
    lexer,
    modules::{self, CrateModules},
    surface,
    use_tree::UsePath,
};
use pretty_assertions::assert_eq;
use std::collections::{BTreeMap, BTreeSet};

fn exposed(source: &str) -> Vec<String> {
    let tokens = lexer::lex(source).tokens;
    let imports = vec![UsePath {
        segments: vec!["dep".to_owned(), "Imported".to_owned()],
        binding: Some("Imported".to_owned()),
    }];
    let surface = surface::scan(&tokens, &imports);
    dbg!(&surface);
    surface.exposed
        .into_iter()
        .filter(|path| path[0] == "dep")
        .map(|path| path.join("::"))
        .collect()
}

#[test]
fn surface_reads_public_signatures_but_not_bodies_or_private_items() {
    let source = r"
        pub fn run(input: dep::Input) -> Result<Imported, dep::Error> { dep::body_only(); }
        pub(crate) fn internal(value: dep::Internal) {}
        fn private(value: dep::Private) {}
        pub struct Holder<T: dep::Bound> { pub field: Vec<dep::Field>, hidden: dep::Hidden }
        pub struct Wrapper(pub dep::Tuple, dep::HiddenTuple);
        pub enum Choice { One(dep::Variant), Two { value: dep::Named } }
        pub trait Port: dep::Super { fn call(&self) -> dep::Output { dep::provided_body() } }
        pub type Alias = dep::Aliased;
        pub const LIMIT: dep::Limit = dep::value();
        pub async unsafe fn qualified(value: dep::Qualified) {}
    ";
    assert_eq!(
        exposed(source),
        [
            "dep::Input",
            "dep::Imported",
            "dep::Error",
            "dep::Bound",
            "dep::Field",
            "dep::Tuple",
            "dep::Variant",
            "dep::Named",
            "dep::Super",
            "dep::Output",
            "dep::Aliased",
            "dep::Limit",
            "dep::Qualified",
        ],
    );
}

#[test]
fn surface_records_reexports_and_extern_crate_aliases() {
    let tokens = lexer::lex("extern crate dep as alias;\npub use dep::{Item, nested::*};\n").tokens;
    let surface = surface::scan(&tokens, &[]);
    dbg!(&surface);
    let path = |segments: &[&str], binding: Option<&str>| UsePath {
        segments: segments
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect(),
        binding: binding.map(str::to_owned),
    };
    assert_eq!(surface.crate_aliases, [path(&["dep"], Some("alias"))]);
    assert_eq!(
        surface.reexported,
        [path(&["dep", "Item"], Some("Item")), path(&["dep", "nested"], None)],
    );
}

/// `app` depends on `api` and `base`; `api` depends on `base` and `other`.
fn workspace() -> (CrateGraph, BTreeMap<String, CrateModules>) {
    let api = concat!(
        "extern crate base as renamed;\n",
        "use other::Shared;\n",
        "pub use other::Shared as Reexported;\n",
        "pub fn make(input: renamed::Input, shared: Shared) -> base::Output { todo!() }\n",
    );
    let tree = MemoryTree(BTreeMap::from([
        ("api/src/lib.rs", api),
        ("app/src/main.rs", "fn main() {}\n"),
    ]));
    let edge = |from: &str, to: &str| Edge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind: DependencyKind::Normal,
    };
    let names = ["api", "app", "base", "other"];
    let graph = CrateGraph {
        crates: names
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        edges: BTreeSet::from([
            edge("app", "api"),
            edge("app", "base"),
            edge("api", "base"),
            edge("api", "other"),
        ]),
        dirs: BTreeMap::from([
            ("api".to_owned(), "api".to_owned()),
            ("app".to_owned(), "app".to_owned()),
        ]),
    };
    let modules = modules::read_workspace(&tree, &graph).unwrap();
    (graph, modules)
}

#[test]
fn a_leak_is_a_dependency_named_in_public_signatures_and_not_reexported() {
    let (graph, modules) = workspace();
    let found = leaks::find(&Reach::new(&graph), &modules);
    dbg!(&found);
    assert_eq!(
        found,
        [Leak {
            crate_name: "api".to_owned(),
            dependency: "base".to_owned(),
            items: BTreeSet::from(["base::Input".to_owned(), "base::Output".to_owned()]),
            shared_dependents: BTreeSet::from(["app".to_owned()]),
        }],
    );
    assert_eq!(leaks::counts(&found), BTreeMap::from([("api".to_owned(), 1)]));
}
