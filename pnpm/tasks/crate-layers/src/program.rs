//! The functions of the workspace, the fields each one accesses, and the
//! functions it calls, read from a rust-analyzer SCIP index.
//!
//! A function accesses a field where its body names the field after a `.`,
//! which reads or assigns it. A struct literal and a pattern name fields
//! too and do not count: every function that builds a type names all of its
//! fields. Functions in test-only modules are left out.

use crate::{
    leaks::lib_name,
    modules::CrateModules,
    scip::{
        self,
        Definitions,
        Index,
        Occurrence,
        Occurrences,
        SymbolInformation,
    },
    tree::Tree,
};
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
        HashMap,
    },
    fmt,
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<Function>,
    pub fields: Vec<Field>,
    /// The path of each type that has a field, such as `pnpm_config::Config`.
    pub types: Vec<String>,
}

/// A top-level module of a crate, the unit that the reports compare.
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Place {
    pub crate_name: String,
    /// Empty for the crate root.
    pub module: String,
}

impl fmt::Display for Place {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&lib_name(&self.crate_name))?;
        if !self.module.is_empty() {
            write!(formatter, "::{}", self.module)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub place: Place,
    pub file: String,
    /// The line of the definition, counted from one.
    pub line: usize,
    /// The fields it accesses, as indexes into [`Program::fields`].
    pub accessed: BTreeSet<usize>,
    /// The functions it names, as indexes into [`Program::functions`]. A
    /// call through a trait names the trait's method, not an implementation.
    pub called: BTreeSet<usize>,
}

impl Function {
    /// `file:line name`.
    pub fn location(&self) -> String {
        format!("{}:{} {}", self.file, self.line, self.name)
    }

    /// A key that sorts functions by where they are.
    pub fn order(&self) -> (&str, usize) {
        (&self.file, self.line)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Field {
    /// The type the field belongs to, as an index into [`Program::types`].
    pub owner: usize,
    pub name: String,
}

/// Reads the functions that `index` defines in the non-test modules of
/// `modules`. Fails when a field is not where the index has it in `tree`,
/// which means the index was made from other sources.
pub fn read(
    index: &Index,
    tree: &dyn Tree,
    modules: &BTreeMap<String, CrateModules>,
) -> Result<Program, String> {
    let mut reader = Reader::new(tree, places(modules));
    for document in &index.documents {
        reader.define(&scip::decode(document)?);
    }
    if reader.program.functions.is_empty() {
        return Err("The index defines no function in a workspace module. Make it with \
                    `rust-analyzer scip` at the repository root."
            .to_owned());
    }
    for document in &index.documents {
        reader.link(&scip::decode(document)?)?;
    }
    Ok(reader.program)
}

/// The place of each file that holds a non-test module.
fn places(modules: &BTreeMap<String, CrateModules>) -> BTreeMap<String, Place> {
    let mut places = BTreeMap::new();
    for (crate_name, crate_modules) in modules {
        for (file, path) in crate_modules.files() {
            let module = path
                .first()
                .cloned()
                .unwrap_or_default();
            places.insert(file.to_owned(), Place { crate_name: crate_name.clone(), module });
        }
    }
    places
}

struct Reader<'tree> {
    tree: &'tree dyn Tree,
    places: BTreeMap<String, Place>,
    program: Program,
    /// The index of each function, field, and type by its SCIP symbol.
    functions: HashMap<String, usize>,
    fields: HashMap<String, usize>,
    types: HashMap<String, usize>,
}

impl<'tree> Reader<'tree> {
    fn new(tree: &'tree dyn Tree, places: BTreeMap<String, Place>) -> Self {
        Self {
            tree,
            places,
            program: Program::default(),
            functions: HashMap::new(),
            fields: HashMap::new(),
            types: HashMap::new(),
        }
    }

    /// Records the functions and fields a document defines.
    fn define(&mut self, definitions: &Definitions) {
        let file = &definitions.relative_path;
        let Some(place) = self.places.get(file).cloned() else { return };
        // A `local` symbol is unique only inside its document.
        let global = definitions.symbols
            .iter()
            .filter(|symbol| !symbol.symbol.starts_with("local "));
        for symbol in global {
            if scip::FUNCTIONS.contains(&symbol.kind) {
                self.define_function(symbol, file.clone(), &place);
            } else if symbol.kind == scip::FIELD {
                self.define_field(symbol);
            }
        }
    }

    fn define_function(&mut self, symbol: &SymbolInformation, file: String, place: &Place) {
        self.functions.insert(symbol.symbol.clone(), self.program.functions.len());
        self.program.functions.push(Function {
            name: symbol.display_name.clone(),
            place: place.clone(),
            file,
            ..Function::default()
        });
    }

    fn define_field(&mut self, symbol: &SymbolInformation) {
        let Some((owner_symbol, type_path)) = field_owner(&symbol.symbol) else { return };
        let types = &mut self.program.types;
        let owner = *self.types
            .entry(owner_symbol.to_owned())
            .or_insert_with(|| {
                types.push(type_path);
                types.len() - 1
            });
        self.fields.insert(symbol.symbol.clone(), self.program.fields.len());
        self.program.fields.push(Field { owner, name: symbol.display_name.clone() });
    }

    /// Records what each function of a document accesses and calls.
    fn link(&mut self, occurrences: &Occurrences) -> Result<(), String> {
        let file = &occurrences.relative_path;
        if !self.places.contains_key(file) {
            return Ok(());
        }
        let source = self.tree
            .read(file)?
            .ok_or_else(|| format!("{file} is missing"))?;
        let lines = source.lines().collect::<Vec<_>>();
        let bodies = self.bodies(&occurrences.occurrences);
        let references = occurrences.occurrences
            .iter()
            .filter(|occurrence| occurrence.symbol_roles & scip::DEFINITION == 0);
        for occurrence in references {
            let Some(function) = innermost(&bodies, occurrence) else { continue };
            self.link_occurrence(function, occurrence, &Source { file, lines: &lines })?;
        }
        Ok(())
    }

    /// The body of each function a document defines, and sets the line of
    /// each one.
    fn bodies(&mut self, occurrences: &[Occurrence]) -> Vec<Body> {
        let mut bodies = Vec::new();
        let definitions = occurrences
            .iter()
            .filter(|occurrence| occurrence.symbol_roles & scip::DEFINITION != 0);
        for occurrence in definitions {
            let Some(&function) = self.functions.get(&occurrence.symbol) else { continue };
            let Some(name) = Span::parse(&occurrence.range).map(|name| name.start) else {
                continue;
            };
            self.program.functions[function].line =
                usize::try_from(name.0).map_or(0, |line| line + 1);
            let item = Span::parse(&occurrence.enclosing_range);
            bodies.extend(item.map(|item| Body { item, name, function }));
        }
        bodies
    }

    fn link_occurrence(
        &mut self,
        function: usize,
        occurrence: &Occurrence,
        source: &Source<'_>,
    ) -> Result<(), String> {
        if let Some(&callee) = self.functions.get(&occurrence.symbol) {
            if callee != function {
                self.program.functions[function].called.insert(callee);
            }
        } else if let Some(&field) = self.fields.get(&occurrence.symbol)
            && source.accesses(&occurrence.range, &self.program.fields[field].name)?
        {
            self.program.functions[function].accessed.insert(field);
        }
        Ok(())
    }
}

/// The symbol and the path of the type that owns the field `symbol`, which
/// reads `<scheme> <manager> <crate> <version> <module>/<Type>#<field>.`
/// with one `<Type>#` more for the field of an enum variant.
fn field_owner(symbol: &str) -> Option<(&str, String)> {
    let owner = &symbol[..=symbol.rfind('#')?];
    let mut parts = owner.splitn(5, ' ');
    let crate_name = parts.nth(2)?;
    let descriptors = parts.nth(1)?;
    let type_path = descriptors
        .rsplit('/')
        .next()?
        .trim_end_matches('#')
        .replace('#', "::");
    Some((owner, format!("{}::{type_path}", lib_name(crate_name))))
}

/// A function of a document.
struct Body {
    /// The whole item. The functions that one macro generates share the
    /// macro's range, such as the methods of an `#[async_trait]` block.
    item: Span,
    /// Where the definition names the function.
    name: (i32, i32),
    function: usize,
}

/// The function that `occurrence` belongs to: the innermost one whose item
/// holds it, and among functions with the same item, the one named last
/// before it.
fn innermost(bodies: &[Body], occurrence: &Occurrence) -> Option<usize> {
    let position = Span::parse(&occurrence.range)?.start;
    bodies
        .iter()
        .filter(|body| body.name <= position && position < body.item.end)
        .max_by_key(|body| (body.item.start, body.name))
        .map(|body| body.function)
}

/// A range of a document, from `start` up to `end`, each a line and a
/// column in bytes counted from zero.
#[derive(Clone, Copy)]
struct Span {
    start: (i32, i32),
    end: (i32, i32),
}

impl Span {
    fn parse(range: &[i32]) -> Option<Self> {
        match *range {
            [line, start, end] => Some(Self { start: (line, start), end: (line, end) }),
            [start_line, start, end_line, end] => {
                Some(Self { start: (start_line, start), end: (end_line, end) })
            }
            _ => None,
        }
    }
}

struct Source<'source> {
    file: &'source str,
    lines: &'source [&'source str],
}

impl Source<'_> {
    /// Whether the field named at `range` follows a `.`. Fails when `range`
    /// does not hold `name`.
    fn accesses(&self, range: &[i32], name: &str) -> Result<bool, String> {
        let stale = || {
            let line = range.first().map_or(0, |line| line + 1);
            format!(
                "{}:{line}: the index has the field `{name}` here and the file does not. Make \
                 the index again for these sources.",
                self.file,
            )
        };
        let &[line, start, end] = range else { return Err(stale()) };
        let text = usize::try_from(line)
            .ok()
            .and_then(|line| self.lines.get(line))
            .ok_or_else(stale)?
            .as_bytes();
        let (start, end) = (start as usize, end as usize);
        if text.get(start..end) != Some(name.as_bytes()) {
            return Err(stale());
        }
        Ok(start > 0 && text[start - 1] == b'.')
    }
}
