use super::MemoryTree;
use crate::{
    graph,
    modules,
    program::{
        self,
        Function,
        Place,
        Program,
    },
    scip::{
        self,
        Definitions,
        Index,
        Occurrence,
        Occurrences,
        SymbolInformation,
    },
};
use pretty_assertions::assert_eq;
use prost::{
    Message,
    bytes::Bytes,
};
use std::collections::BTreeMap;

const ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"crates/*\"]\n";
const LIB: &str = "pub mod left;\npub mod right;\npub mod state;\n#[cfg(test)]\nmod tests;\n";
const STATE: &str =
    "pub struct State { pub alpha: u8, pub beta: u8 }\npub enum Choice { Named { gamma: u8 } }\n";
const LEFT: &str = "\
pub fn read(state: &State) -> u8 { state.alpha + helper(state) }
fn helper(state: &State) -> u8 { state.beta }
";
const RIGHT: &str = "\
pub fn build() -> State { State { alpha: 1, beta: 2 } }
pub fn outer(state: &State, choice: &Choice) -> u8 {
    fn inner(state: &State) -> u8 { state.beta }
    let Choice::Named { gamma } = choice;
    inner(state) + state.alpha + gamma
}
";
const TESTS: &str = "fn check(state: &State) -> u8 { state.alpha }\n";

fn symbol(descriptors: &str) -> String {
    format!("rust-analyzer cargo one 0.0.0 {descriptors}")
}

/// One document of an index, as rust-analyzer would write it for `source`.
struct Document {
    path: &'static str,
    source: &'static str,
    symbols: Vec<SymbolInformation>,
    occurrences: Vec<Occurrence>,
}

impl Document {
    fn new(path: &'static str, source: &'static str) -> Self {
        Self { path, source, symbols: Vec::new(), occurrences: Vec::new() }
    }

    /// Defines the function `name`, whose item runs from the line that
    /// holds `first_line` to the end of the line that holds `last_line`.
    fn function(mut self, descriptors: &str, name: &str, item: (&str, &str)) -> Self {
        let (line, column) = self
            .positions(name)
            .into_iter()
            .find(|(line, column)| self.line(*line)[..*column].ends_with("fn "))
            .unwrap();
        let line_of = |marker: &str| {
            self.source
                .lines()
                .position(|text| text.contains(marker))
                .unwrap()
        };
        let (start_line, end_line) = (line_of(item.0), line_of(item.1));
        self.symbols.push(SymbolInformation {
            symbol: symbol(descriptors),
            kind: scip::FUNCTIONS[0],
            display_name: name.to_owned(),
        });
        self.occurrences.push(Occurrence {
            range: vec![line, column as i32, (column + name.len()) as i32],
            symbol: symbol(descriptors),
            symbol_roles: scip::DEFINITION,
            enclosing_range: vec![
                start_line as i32,
                0,
                end_line as i32,
                self.line(end_line as i32).len() as i32,
            ],
        });
        self
    }

    fn field(mut self, descriptors: &str, name: &str) -> Self {
        self.symbols.push(SymbolInformation {
            symbol: symbol(descriptors),
            kind: scip::FIELD,
            display_name: name.to_owned(),
        });
        self
    }

    /// Names `descriptors` wherever the source holds the word `name`,
    /// except in `fn name`.
    fn references(mut self, descriptors: &str, name: &str) -> Self {
        for (line, column) in self.positions(name) {
            if self.line(line)[..column].ends_with("fn ") {
                continue;
            }
            self.occurrences.push(Occurrence {
                range: vec![line, column as i32, (column + name.len()) as i32],
                symbol: symbol(descriptors),
                symbol_roles: 0,
                enclosing_range: Vec::new(),
            });
        }
        self
    }

    fn line(&self, line: i32) -> &'static str {
        self.source
            .lines()
            .nth(line as usize)
            .unwrap()
    }

    /// The line and column of each place the source holds the word `name`.
    fn positions(&self, name: &str) -> Vec<(i32, usize)> {
        let is_word = |text: &str, column: usize| {
            let part_of_word = |character: char| character == '_' || character.is_alphanumeric();
            !text[..column].ends_with(part_of_word)
                && !text[column + name.len()..].starts_with(part_of_word)
        };
        let mut positions = Vec::new();
        for (line, text) in self.source.lines().enumerate() {
            let columns = text.match_indices(name).map(|(column, _)| column);
            positions.extend(
                columns
                    .filter(|column| is_word(text, *column))
                    .map(|column| (line as i32, column)),
            );
        }
        positions
    }

    /// The document's bytes. An encoded message followed by another of the
    /// same type decodes as the two merged, so the two views make one
    /// `Document`.
    fn encode(self) -> Bytes {
        let definitions =
            Definitions { relative_path: self.path.to_owned(), symbols: self.symbols };
        let occurrences =
            Occurrences { relative_path: self.path.to_owned(), occurrences: self.occurrences };
        let mut bytes = definitions.encode_to_vec();
        bytes.extend(occurrences.encode_to_vec());
        Bytes::from(bytes)
    }
}

fn state_document() -> Document {
    Document::new("crates/one/src/state.rs", STATE)
        .field("state/State#alpha.", "alpha")
        .field("state/State#beta.", "beta")
        .field("state/Choice#Named#gamma.", "gamma")
}

fn left_document(source: &'static str) -> Document {
    Document::new("crates/one/src/left.rs", source)
        .function("left/read().", "read", ("pub fn read", "pub fn read"))
        .function("left/helper().", "helper", ("fn helper", "fn helper"))
        .references("left/helper().", "helper")
        .references("state/State#alpha.", "alpha")
        .references("state/State#beta.", "beta")
}

fn right_document() -> Document {
    Document::new("crates/one/src/right.rs", RIGHT)
        .function("right/build().", "build", ("pub fn build", "pub fn build"))
        .function("right/outer().", "outer", ("pub fn outer", "inner(state) +"))
        .function("right/outer().inner().", "inner", ("fn inner", "fn inner"))
        .references("right/outer().inner().", "inner")
        .references("state/State#alpha.", "alpha")
        .references("state/State#beta.", "beta")
        .references("state/Choice#Named#gamma.", "gamma")
}

fn tests_document() -> Document {
    Document::new("crates/one/src/tests.rs", TESTS)
        .function("tests/check().", "check", ("fn check", "fn check"))
        .references("state/State#alpha.", "alpha")
}

fn read(documents: Vec<Document>, left_source: &'static str) -> Result<Program, String> {
    let tree = MemoryTree(BTreeMap::from([
        ("Cargo.toml", ROOT_MANIFEST),
        ("crates/one/Cargo.toml", "[package]\nname = \"one\"\n"),
        ("crates/one/src/lib.rs", LIB),
        ("crates/one/src/left.rs", left_source),
        ("crates/one/src/right.rs", RIGHT),
        ("crates/one/src/state.rs", STATE),
        ("crates/one/src/tests.rs", TESTS),
    ]));
    let modules = modules::read_workspace(&tree, &graph::read(&tree).unwrap()).unwrap();
    let index = Index {
        documents: documents
            .into_iter()
            .map(Document::encode)
            .collect(),
    };
    program::read(&index, &tree, &modules)
}

type Summary<'program> = (String, &'program str, usize, Vec<String>, Vec<&'program str>);

/// The place, name, line, accessed fields, and called functions of each
/// function.
fn summary(program: &Program) -> Vec<Summary<'_>> {
    program.functions
        .iter()
        .map(|function| describe(program, function))
        .collect()
}

fn describe<'program>(
    program: &'program Program,
    function: &'program Function,
) -> Summary<'program> {
    let field = |index: &usize| {
        let field = &program.fields[*index];
        format!("{}.{}", program.types[field.owner], field.name)
    };
    let called = function.called
        .iter()
        .map(|callee| program.functions[*callee].name.as_str());
    (
        function.place.to_string(),
        function.name.as_str(),
        function.line,
        function.accessed
            .iter()
            .map(field)
            .collect(),
        called.collect(),
    )
}

#[test]
fn reads_the_fields_a_function_accesses_and_the_functions_it_calls() {
    let program = read(vec![state_document(), left_document(LEFT)], LEFT).unwrap();
    dbg!(&program);
    assert_eq!(
        summary(&program),
        vec![
            (
                "one::left".to_owned(),
                "read",
                1,
                vec!["one::state::State.alpha".to_owned()],
                vec!["helper"]
            ),
            (
                "one::left".to_owned(),
                "helper",
                2,
                vec!["one::state::State.beta".to_owned()],
                vec![]
            ),
        ],
    );
}

#[test]
fn leaves_out_fields_named_in_struct_literals_and_patterns() {
    let program = read(vec![state_document(), right_document()], LEFT).unwrap();
    dbg!(&program);
    let accessed = summary(&program)
        .into_iter()
        .map(|(_, name, _, accessed, _)| (name, accessed))
        .collect::<Vec<_>>();
    assert_eq!(
        accessed,
        vec![
            ("build", vec![]),
            ("outer", vec!["one::state::State.alpha".to_owned()]),
            ("inner", vec!["one::state::State.beta".to_owned()]),
        ],
    );
}

#[test]
fn gives_a_reference_to_the_innermost_function_that_holds_it() {
    let program = read(vec![state_document(), right_document()], LEFT).unwrap();
    dbg!(&program);
    let outer = &program.functions[1];
    assert_eq!((outer.name.as_str(), outer.line), ("outer", 2));
    let called = outer.called
        .iter()
        .map(|callee| program.functions[*callee].name.as_str());
    assert_eq!(called.collect::<Vec<_>>(), vec!["inner"]);
}

#[test]
fn splits_the_item_of_a_macro_between_the_functions_it_generates() {
    let source = "\
#[async_trait]
impl Port for State {
    async fn first(&self) -> u8 { self.alpha }
    async fn second(&self) -> u8 { self.beta }
}
";
    let item = ("#[async_trait]", "async fn second");
    let document = Document::new("crates/one/src/left.rs", source)
        .function("left/impl#[State][Port]first().", "first", item)
        .function("left/impl#[State][Port]second().", "second", item)
        .references("state/State#alpha.", "alpha")
        .references("state/State#beta.", "beta");
    let program = read(vec![state_document(), document], source).unwrap();
    dbg!(&program);
    let accessed = summary(&program)
        .into_iter()
        .map(|(_, name, _, accessed, _)| (name, accessed))
        .collect::<Vec<_>>();
    assert_eq!(
        accessed,
        vec![
            ("first", vec!["one::state::State.alpha".to_owned()]),
            ("second", vec!["one::state::State.beta".to_owned()])
        ],
    );
}

#[test]
fn leaves_out_functions_in_test_only_modules() {
    let program =
        read(vec![state_document(), left_document(LEFT), tests_document()], LEFT).unwrap();
    dbg!(&program);
    let names = program.functions.iter().map(|function| function.name.as_str());
    assert_eq!(names.collect::<Vec<_>>(), vec!["read", "helper"]);
}

#[test]
fn places_a_function_in_its_crate_root_or_top_level_module() {
    let root = Place { crate_name: "pnpm-one".to_owned(), module: String::new() };
    let module = Place { crate_name: "pnpm-one".to_owned(), module: "left".to_owned() };
    assert_eq!(
        (root.to_string(), module.to_string()),
        ("pnpm_one".to_owned(), "pnpm_one::left".to_owned()),
    );
}

#[test]
fn fails_when_a_source_changed_after_the_index_was_made() {
    let changed = "\
pub fn read(state: &State) -> u8 { helper(state) + state.alpha }
fn helper(state: &State) -> u8 { state.beta }
";
    let error = read(vec![state_document(), left_document(LEFT)], changed).unwrap_err();
    dbg!(&error);
    assert!(error.starts_with("crates/one/src/left.rs:1: the index has the field `alpha` here"));
}

#[test]
fn fails_when_the_index_covers_no_workspace_module() {
    let elsewhere = Document::new("src/left.rs", LEFT)
        .function("left/read().", "read", ("pub fn read", "pub fn read"));
    let error = read(vec![elsewhere], LEFT).unwrap_err();
    dbg!(&error);
    assert!(error.starts_with("The index defines no function in a workspace module."));
}
