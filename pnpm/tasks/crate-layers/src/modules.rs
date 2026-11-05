//! The module tree of one crate and the references between its top-level
//! modules, read from source without compiling it.
//!
//! Test-only modules (`#[cfg(test)] mod …;`) are left out. Inline modules
//! (`mod name { … }`) count as part of the module that contains them.

use crate::{
    cfg,
    graph::CrateGraph,
    lexer::{self, Token},
    surface::{self, Surface},
    tree::Tree,
    use_tree::{self, UsePath},
};
use std::collections::BTreeMap;

pub type ModulePath = Vec<String>;

/// How many levels of `use` re-exports a path is followed through.
const MAX_REEXPORT_DEPTH: u8 = 4;

#[derive(Debug, Default)]
struct Module {
    code_lines: usize,
    imports: Vec<UsePath>,
    references: Vec<Vec<String>>,
    surface: Surface,
}

#[derive(Debug, Default)]
pub struct CrateModules {
    modules: BTreeMap<ModulePath, Module>,
}

/// The top-level modules of a crate, their sizes, and how often each one
/// names another.
#[derive(Debug, Default)]
pub struct TopLevelGraph {
    pub code_lines: BTreeMap<String, usize>,
    pub edges: BTreeMap<(String, String), usize>,
}

/// The modules of every workspace crate, by crate name.
pub fn read_workspace(
    tree: &dyn Tree,
    graph: &CrateGraph,
) -> Result<BTreeMap<String, CrateModules>, String> {
    graph.dirs
        .iter()
        .map(|(name, dir)| Ok((name.clone(), CrateModules::read(tree, dir)?)))
        .collect()
}

impl CrateModules {
    /// Reads the crate whose manifest is in `dir`, starting at `src/lib.rs`,
    /// or at `src/main.rs` for a binary-only crate.
    pub fn read(tree: &dyn Tree, dir: &str) -> Result<Self, String> {
        let mut crate_modules = Self::default();
        for root in ["src/lib.rs", "src/main.rs"] {
            let file = format!("{dir}/{root}");
            if let Some(source) = tree.read(&file)? {
                crate_modules.visit(tree, Vec::new(), &file, &source)?;
                break;
            }
        }
        Ok(crate_modules)
    }

    fn visit(
        &mut self,
        tree: &dyn Tree,
        path: ModulePath,
        file: &str,
        source: &str,
    ) -> Result<(), String> {
        let lexed = lexer::lex(source);
        let scan = scan(&lexed.tokens);
        let child_dir = child_dir(file);
        for child in &scan.children {
            let (child_file, child_source) = read_child(tree, &child_dir, child)?.ok_or_else(|| {
                format!("{file} declares `mod {child};`, but neither {child_dir}/{child}.rs nor {child_dir}/{child}/mod.rs exists")
            })?;
            let mut child_path = path.clone();
            child_path.push(child.clone());
            self.visit(tree, child_path, &child_file, &child_source)?;
        }
        let module = Module {
            code_lines: lexed.code_lines,
            surface: surface::scan(&lexed.tokens),
            imports: scan.imports,
            references: scan.references,
        };
        self.modules.insert(path, module);
        Ok(())
    }

    /// The public surface of each non-test module.
    pub fn surfaces(&self) -> impl Iterator<Item = &Surface> {
        self.modules.values().map(|module| &module.surface)
    }

    /// The surface of the crate root, whose `extern crate` aliases bind in
    /// every module.
    pub fn root_surface(&self) -> Option<&Surface> {
        self.modules
            .get(&ModulePath::new())
            .map(|module| &module.surface)
    }

    pub fn top_level_graph(&self) -> TopLevelGraph {
        let mut graph = TopLevelGraph::default();
        for (path, module) in &self.modules {
            let Some(from) = path.first() else { continue };
            *graph.code_lines.entry(from.clone()).or_default() += module.code_lines;
            for reference in module_references(module) {
                let Some(to) = self
                    .resolve(path, reference, 0)
                    .and_then(|target| target.first().cloned())
                else {
                    continue;
                };
                if &to != from {
                    *graph.edges
                        .entry((from.clone(), to))
                        .or_default() += 1;
                }
            }
        }
        graph
    }

    /// The module a path written in `from` points into: the deepest module
    /// it names, following `use` re-exports for the first name that is not
    /// a module.
    fn resolve(&self, from: &[String], segments: &[String], depth: u8) -> Option<ModulePath> {
        let (mut module, rest) = self.path_start(from, segments, depth)?;
        for segment in rest {
            module.push(segment.clone());
            if self.modules.contains_key(&module) {
                continue;
            }
            module.pop();
            return self
                .resolve_import(&module, segment, depth)
                .or(Some(module));
        }
        Some(module)
    }

    fn path_start<'path>(
        &self,
        from: &[String],
        segments: &'path [String],
        depth: u8,
    ) -> Option<(ModulePath, &'path [String])> {
        let (head, rest) = segments.split_first()?;
        match head.as_str() {
            "crate" => Some((Vec::new(), rest)),
            "self" => Some((from.to_vec(), rest)),
            "super" => {
                let supers = segments
                    .iter()
                    .take_while(|segment| *segment == "super")
                    .count();
                let kept = from.len().checked_sub(supers)?;
                Some((from[..kept].to_vec(), &segments[supers..]))
            }
            _ => {
                let mut child = from.to_vec();
                child.push(head.clone());
                if self.modules.contains_key(&child) {
                    return Some((child, rest));
                }
                Some((self.resolve_import(from, head, depth)?, rest))
            }
        }
    }

    fn resolve_import(&self, module: &[String], name: &str, depth: u8) -> Option<ModulePath> {
        if depth >= MAX_REEXPORT_DEPTH {
            return None;
        }
        let imports = &self.modules.get(module)?.imports;
        let import = imports
            .iter()
            .find(|import| import.binding.as_deref() == Some(name))?;
        self.resolve(module, &import.segments, depth + 1)
    }
}

fn module_references(module: &Module) -> impl Iterator<Item = &[String]> {
    let imports = module.imports.iter().map(|import| import.segments.as_slice());
    imports.chain(module.references.iter().map(Vec::as_slice))
}

#[derive(Debug, Default)]
struct Scan {
    children: Vec<String>,
    imports: Vec<UsePath>,
    references: Vec<Vec<String>>,
}

/// Collects the non-test `mod name;` declarations, the `use` items, and the
/// paths of a file.
fn scan(tokens: &[Token]) -> Scan {
    let mut scan = Scan::default();
    let mut pos = 0;
    while pos < tokens.len() {
        pos = scan_at(tokens, pos, &mut scan);
    }
    scan
}

/// Reads the item or path at `pos` and returns where scanning continues.
fn scan_at(tokens: &[Token], pos: usize, scan: &mut Scan) -> usize {
    let Token::Ident(word) = &tokens[pos] else { return pos + 1 };
    // `use<'a>` in an `impl Trait + use<'a>` bound is not a `use` item.
    if word == "use" && tokens.get(pos + 1) != Some(&Token::Punct('<')) {
        let (paths, next) = use_tree::parse(tokens, pos + 1);
        scan.imports.extend(paths);
        return next;
    }
    if word == "mod" {
        let name = declared_child(tokens, pos);
        if let Some(name) = name.filter(|name| !scan.children.contains(name)) {
            scan.children.push(name);
        }
        return pos + 1;
    }
    let starts_path = pos == 0 || tokens[pos - 1] != Token::PathSep;
    if starts_path && tokens.get(pos + 1) == Some(&Token::PathSep) {
        let path = read_path(tokens, pos);
        let next = pos + path.len() * 2 - 1;
        scan.references.push(path);
        return next;
    }
    pos + 1
}

/// The name in `mod name;` at `tokens[pos]`, unless a `#[cfg(test)]`-like
/// attribute right before it makes the module test-only.
fn declared_child(tokens: &[Token], pos: usize) -> Option<String> {
    let Some(Token::Ident(name)) = tokens.get(pos + 1) else { return None };
    if tokens.get(pos + 2) != Some(&Token::Punct(';')) || cfg::is_test_only(tokens, pos) {
        return None;
    }
    Some(name.clone())
}

/// The segments of the path `ident (:: ident)*` that starts at `start`.
fn read_path(tokens: &[Token], start: usize) -> Vec<String> {
    let mut path = Vec::new();
    let mut pos = start;
    while let Some(Token::Ident(segment)) = tokens.get(pos) {
        path.push(segment.clone());
        if tokens.get(pos + 1) != Some(&Token::PathSep) {
            break;
        }
        pos += 2;
    }
    path
}

/// The directory that holds the files of a module's children: the file's
/// own directory for `lib.rs`, `main.rs`, and `mod.rs`, and the directory
/// named after the file otherwise.
fn child_dir(file: &str) -> String {
    let (parent, name) = file
        .rsplit_once('/')
        .unwrap_or(("", file));
    match name {
        "lib.rs" | "main.rs" | "mod.rs" => parent.to_owned(),
        _ => file.trim_end_matches(".rs").to_owned(),
    }
}

fn read_child(tree: &dyn Tree, dir: &str, child: &str) -> Result<Option<(String, String)>, String> {
    for file in [format!("{dir}/{child}.rs"), format!("{dir}/{child}/mod.rs")] {
        if let Some(source) = tree.read(&file)? {
            return Ok(Some((file, source)));
        }
    }
    Ok(None)
}
