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
    let surface = surface::scan(&lexer::lex(source).tokens);
    dbg!(&surface);
    surface.exposed
        .into_iter()
        .chain(surface.resolved.exposed)
        .filter(|path| path[0] == "dep")
        .map(|path| path.join("::"))
        .collect()
}

#[test]
fn surface_reads_public_signatures_but_not_bodies_or_private_items() {
    let source = r#"
        use dep::Imported;
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
        pub const unsafe fn const_unsafe(value: dep::ConstUnsafe) { dep::const_unsafe_body() }
        pub const extern "C" fn const_extern(value: dep::ConstExtern) { dep::const_extern_body() }
        pub const r#unsafe: dep::RawConst = dep::raw_const_value();
        pub trait Defaults { const LIMIT: dep::Limit2 = dep::default_limit(); fn after(&self) -> dep::After; }
        pub trait Bounded { type Items: Iterator<Item = dep::BoundItem>; fn last(&self) -> dep::Last; }
        pub type Api = Wrapper<{ N }, dep::ConstArg>;
        pub const EMPTY: Option<Box<dyn Iterator<Item = dep::AssocItem>>> = dep::empty_value();
        pub trait ArrayBound { type Items: Bound<Item = [u8; 1], Other = dep::AfterArray>; }
        pub trait Shifted { const MASK: usize = 1 << 3; const LIMIT: usize = dep::shifted_default(); fn wrapped() -> outer::Wrapper::<dep::Turbofish>; }
    "#;
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
            "dep::ConstUnsafe",
            "dep::ConstExtern",
            "dep::RawConst",
            "dep::Limit2",
            "dep::After",
            "dep::BoundItem",
            "dep::Last",
            "dep::ConstArg",
            "dep::AssocItem",
            "dep::AfterArray",
            "dep::Turbofish",
        ],
    );
}

#[test]
fn surface_skips_test_only_items_and_their_imports() {
    let source = concat!(
        "#[cfg(test)]\n",
        "mod tests { pub fn helper(value: dep::TestOnly) {} pub use dep::Shadow; }\n",
        "#[cfg(test)] pub use dep::Hidden;\n",
        "#[cfg(test)] pub fn test_helper(value: dep::TestFn) {}\n",
        "#[cfg(test)] use dep::Local;\n",
        "pub struct Shadow;\n",
        "pub struct Local;\n",
        "pub fn after(value: dep::After, shadow: Shadow, local: Local) {}\n",
        "pub struct Probed { #[cfg(test)] pub probe: Option<dep::Probe<'static, u8>>, pub kept: dep::Kept }\n",
        "pub enum Following { Variant(dep::Variant) }\n",
        "#[cfg(test)]\n",
        "impl<A, B> Following where A: Clone, B: Copy { pub fn probe(&self) -> dep::TestImpl { todo!() } }\n",
        "#[cfg(test)] pub(crate) fn scoped(value: dep::Scoped) where u8: Copy, u16: Copy {}\n",
        "#[cfg(test)] const MASK: usize = 1 << 3;\n",
        "pub fn last(value: dep::Last) {}\n",
    );
    let surface = surface::scan(&lexer::lex(source).tokens);
    dbg!(&surface);
    let names = surface.exposed
        .iter()
        .filter(|path| path[0] == "dep")
        .map(|path| path.join("::"))
        .collect::<Vec<_>>();
    assert_eq!(names, ["dep::After", "dep::Kept", "dep::Variant", "dep::Last"]);
    assert_eq!(surface.reexported, []);
}

#[test]
fn surface_expands_a_name_through_its_first_import() {
    let source = concat!(
        "pub use first::Client;\n",
        "fn helper() { use second::Client; }\n",
        "pub fn make() -> Client { todo!() }\n",
    );
    let surface = surface::scan(&lexer::lex(source).tokens);
    dbg!(&surface);
    let clients = surface.exposed
        .iter()
        .filter(|path| {
            path.last()
                .is_some_and(|name| name == "Client")
        })
        .map(|path| path.join("::"))
        .collect::<Vec<_>>();
    assert_eq!(clients, ["first::Client"]);
}

#[test]
fn surface_scopes_imports_to_their_module_and_block() {
    let source = concat!(
        "pub mod nested { use dep::Thing; fn inner(_: Thing) {} }\n",
        "fn helper() { use dep::Client; }\n",
        "mod scoped { use dep::Inner; pub fn reaches(_: Inner) {} }\n",
        "use dep::module;\n",
        "pub use module::Reexported;\n",
        "pub struct Thing;\n",
        "pub struct Client;\n",
        "pub fn outer(thing: Thing, client: Client, reexported: module::Reexported) {}\n",
    );
    assert_eq!(exposed(source), ["dep::module::Reexported", "dep::Inner"]);
    let surface = surface::scan(&lexer::lex(source).tokens);
    let reexported = surface.reexported
        .iter()
        .map(|reexport| reexport.segments.join("::"))
        .collect::<Vec<_>>();
    assert_eq!(reexported, ["dep::module::Reexported"]);
}

#[test]
fn surface_skips_test_only_unions_aliases_and_trait_members() {
    let source = concat!(
        "#[cfg(test)] extern crate dep as test_alias;\n",
        "#[cfg(test)]\n",
        "pub union Hidden<T, U> where T: Copy, U: Copy { pub field: dep::UnionField }\n",
        "pub struct Bag { pub bytes: [u8; 1 << 3], hidden: dep::Hidden }\n",
        "pub trait Probe { #[cfg(test)] fn probe(value: dep::TraitTest); fn real(&self) -> dep::TraitReal; }\n",
    );
    assert_eq!(exposed(source), ["dep::TraitReal"]);
    let surface = surface::scan(&lexer::lex(source).tokens);
    assert_eq!(surface.crate_aliases, []);
}

#[test]
fn surface_records_reexports_and_extern_crate_aliases() {
    let tokens = lexer::lex("extern crate dep as alias;\npub use dep::{Item, nested::*};\n").tokens;
    let surface = surface::scan(&tokens);
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

fn workspace() -> (CrateGraph, BTreeMap<String, CrateModules>) {
    let api = concat!(
        "extern crate base as renamed;\n",
        "extern crate other as aliased;\n",
        "use other::Shared;\n",
        "pub use aliased::Shared as Reexported;\n",
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
fn a_module_sees_its_own_extern_crate_aliases_before_the_roots() {
    let tree = MemoryTree(BTreeMap::from([
        (
            "api/src/lib.rs",
            "extern crate base as renamed;\nextern crate base as other;\nmod child;\nmod plain;\n",
        ),
        (
            "api/src/child.rs",
            "extern crate other as renamed;\npub fn shadowed(_: renamed::Item) {}\n",
        ),
        (
            "api/src/plain.rs",
            concat!(
                "pub fn rooted(_: renamed::RootItem) {}\n",
                "mod inline { extern crate other as renamed; pub fn once(_: renamed::Once) {} }\n",
                "mod uses_root { pub fn inherited(_: renamed::Inherited) {} }\n",
            ),
        ),
    ]));
    let edge =
        |to: &str| Edge { from: "api".to_owned(), to: to.to_owned(), kind: DependencyKind::Normal };
    let graph = CrateGraph {
        crates: ["api", "base", "other"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        edges: BTreeSet::from([edge("base"), edge("other")]),
        dirs: BTreeMap::from([("api".to_owned(), "api".to_owned())]),
    };
    let modules = modules::read_workspace(&tree, &graph).unwrap();
    let found = leaks::find(&Reach::new(&graph), &modules);
    dbg!(&found);
    let items = found
        .iter()
        .flat_map(|leak| leak.items.iter().cloned())
        .collect::<Vec<_>>();
    assert_eq!(items, ["base::Inherited", "base::RootItem", "other::Item", "other::Once"]);
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
