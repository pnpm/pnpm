//! The paths a file's public items name in their signatures, read from the
//! token stream. Visibility is taken from the `pub` keyword alone: a `pub`
//! item inside a private module counts as public.

use crate::{
    cfg,
    extent::{Nesting, Stop, closing_brace, item_end},
    lexer::Token,
    use_tree::{self, UsePath},
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Surface {
    /// Paths named in public signatures, with a first segment that a `use`
    /// in the same scope imports replaced by the imported path.
    pub exposed: Vec<Vec<String>>,
    /// The paths of `pub use` items.
    pub reexported: Vec<UsePath>,
    /// `extern crate name as alias;` items. An alias binds in its own module,
    /// and in the whole crate when the crate root declares it.
    pub crate_aliases: Vec<UsePath>,
    /// Exposed and re-exported paths whose first segment an inline module's
    /// own `extern crate` alias already replaced. No outer alias applies to
    /// them.
    pub resolved: Resolved,
}

impl Surface {
    /// Moves the paths that an inline module's own `extern crate` aliases
    /// bind into `resolved`, with each alias replaced, and drops the aliases.
    /// The other paths stay for the enclosing scopes' aliases.
    fn resolve_own_aliases(&mut self) {
        let owned = std::mem::take(&mut self.crate_aliases);
        let aliases = owned.iter().collect::<Vec<_>>();
        let binds = |path: &[String]| {
            aliases
                .iter()
                .any(|alias| alias.binding.as_ref() == path.first())
        };
        let (aliased, plain) = std::mem::take(&mut self.exposed)
            .into_iter()
            .partition::<Vec<_>, _>(|path| binds(path));
        self.exposed = plain;
        let resolved = aliased.iter().map(|path| dealias(path, &aliases));
        self.resolved.exposed.extend(resolved);
        let (aliased, plain) = std::mem::take(&mut self.reexported)
            .into_iter()
            .partition::<Vec<_>, _>(|reexport| binds(&reexport.segments));
        self.reexported = plain;
        let resolved = aliased.iter().map(|reexport| dealias(&reexport.segments, &aliases));
        self.resolved.reexported.extend(resolved);
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Resolved {
    pub exposed: Vec<Vec<String>>,
    pub reexported: Vec<Vec<String>>,
}

/// The surface of one file. Test-only items, including their imports, are
/// left out. An inline `mod name { … }` is a scope of its own: its imports
/// and aliases apply only inside it.
pub fn scan(tokens: &[Token]) -> Surface {
    let mut scope = Scope { tokens, ..Scope::default() };
    let mut pos = 0;
    while pos < tokens.len() {
        pos = scope.step(pos).max(pos + 1);
    }
    scope.finish()
}

/// Replaces a first segment that an `extern crate` alias binds with the
/// crate's name.
pub fn dealias(path: &[String], aliases: &[&UsePath]) -> Vec<String> {
    let alias = aliases
        .iter()
        .find(|alias| alias.binding.as_ref() == path.first());
    let Some(alias) = alias else { return path.to_vec() };
    alias.segments
        .iter()
        .chain(&path[1..])
        .cloned()
        .collect()
}

/// The state of one scope's scan.
#[derive(Default)]
struct Scope<'tokens> {
    tokens: &'tokens [Token],
    surface: Surface,
    imports: Vec<UsePath>,
    /// The surfaces of inline modules, already expanded through their own
    /// imports and aliases.
    nested: Vec<Surface>,
    /// The depth of `{` blocks: a `use` inside a function body or another
    /// block is not one of the scope's imports.
    braces: usize,
}

impl Scope<'_> {
    /// Reads what starts at `pos` and returns where scanning continues.
    fn step(&mut self, pos: usize) -> usize {
        let tokens = self.tokens;
        if let Some(end) = test_only_item_end(tokens, pos) {
            return end;
        }
        if let Some(end) = self.inline_module(pos) {
            return end;
        }
        if let Some(alias) = extern_crate_alias(tokens, pos) {
            self.surface.crate_aliases.push(alias);
        }
        match tokens[pos] {
            Token::Punct('{') => self.braces += 1,
            Token::Punct('}') => self.braces = self.braces.saturating_sub(1),
            _ => {}
        }
        if let Some(item) = use_item(tokens, pos) {
            return self.record_use(item);
        }
        scan_at(tokens, pos, &mut self.surface)
    }

    /// Scans the inline module `mod name { … }` at `pos` as its own scope.
    fn inline_module(&mut self, pos: usize) -> Option<usize> {
        let tokens = self.tokens;
        let opens = is_ident(tokens.get(pos), "mod")
            && matches!(tokens.get(pos + 1), Some(Token::Ident(_)))
            && tokens.get(pos + 2) == Some(&Token::Punct('{'));
        if !opens {
            return None;
        }
        let end = closing_brace(tokens, pos + 2);
        let body = tokens
            .get(pos + 3..end.saturating_sub(1))
            .unwrap_or_default();
        let mut inner = scan(body);
        inner.resolve_own_aliases();
        self.nested.push(inner);
        Some(end)
    }

    /// Adds a `use` item at the scope's top level to its imports, and a
    /// `pub use` to its re-exports, after expanding each path through the
    /// imports before it. Returns the index after the item.
    fn record_use(&mut self, item: UseItem) -> usize {
        if self.braces > 0 {
            return item.next;
        }
        for mut path in item.paths {
            expand(&mut path.segments, &self.imports);
            if item.is_public {
                self.surface.reexported.push(path.clone());
            }
            self.imports.push(path);
        }
        item.next
    }

    fn finish(mut self) -> Surface {
        for path in &mut self.surface.exposed {
            expand(path, &self.imports);
        }
        for inner in self.nested {
            self.surface.exposed.extend(inner.exposed);
            self.surface.reexported.extend(inner.reexported);
            self.surface.resolved.exposed.extend(inner.resolved.exposed);
            self.surface.resolved.reexported.extend(inner.resolved.reexported);
        }
        self.surface
    }
}

/// The keywords an item starts with, after its visibility and qualifiers.
const ITEM_KEYWORDS: [&str; 13] = [
    "use", "mod", "fn", "struct", "enum", "union", "trait", "impl", "const", "static", "type",
    "extern", "crate",
];

/// The index after the test-only item or field that starts at `pos`, if one
/// does. An item ends at its `;` or body. A field also ends at its `,`, which
/// in an item's header separates generics or `where` predicates instead.
fn test_only_item_end(tokens: &[Token], pos: usize) -> Option<usize> {
    let is_keyword = |index: usize| matches!(tokens.get(index), Some(Token::Ident(word)) if ITEM_KEYWORDS.contains(&word.as_str()));
    let starts = is_keyword(pos) || is_ident(tokens.get(pos), "pub");
    if !starts || !cfg::is_test_only(tokens, pos) {
        return None;
    }
    let stop = if is_keyword(skip_qualifiers(tokens, skip_visibility(tokens, pos))) {
        Stop::ItemEnd
    } else {
        Stop::Extent
    };
    Some(item_end(tokens, pos, stop))
}

/// The index after `pub` or `pub(…)` at `pos`, or `pos` when there is none.
fn skip_visibility(tokens: &[Token], pos: usize) -> usize {
    if !is_ident(tokens.get(pos), "pub") {
        return pos;
    }
    if tokens.get(pos + 1) != Some(&Token::Punct('(')) {
        return pos + 1;
    }
    let close = tokens
        .iter()
        .skip(pos + 1)
        .position(|token| *token == Token::Punct(')'));
    close
        .map(|offset| pos + offset + 2)
        .unwrap_or(pos + 1)
}

/// A `use` item: its paths, whether it is `pub use`, and the index after it.
struct UseItem {
    paths: Vec<UsePath>,
    is_public: bool,
    next: usize,
}

/// The `use` or `pub use` item at `pos`, if one starts there. `use<'a>` in an
/// `impl Trait + use<'a>` bound is not a `use` item.
fn use_item(tokens: &[Token], pos: usize) -> Option<UseItem> {
    let is_public = is_ident(tokens.get(pos), "pub") && is_ident(tokens.get(pos + 1), "use");
    let keyword = if is_public { pos + 1 } else { pos };
    let is_use =
        is_ident(tokens.get(keyword), "use") && tokens.get(keyword + 1) != Some(&Token::Punct('<'));
    if !is_use {
        return None;
    }
    let (paths, next) = use_tree::parse(tokens, keyword + 1);
    Some(UseItem { paths, is_public, next })
}

/// The `extern crate name as alias;` at `pos`, if there is one.
fn extern_crate_alias(tokens: &[Token], pos: usize) -> Option<UsePath> {
    let [extern_, crate_, Token::Ident(name), as_, Token::Ident(alias)] = tokens.get(pos..pos + 5)?
    else {
        return None;
    };
    let keywords = [(extern_, "extern"), (crate_, "crate"), (as_, "as")];
    keywords
        .iter()
        .all(|(token, word)| is_ident(Some(token), word))
        .then(|| UsePath { segments: vec![name.clone()], binding: Some(alias.clone()) })
}

/// Reads the public item at `pos`, if one starts there, and returns where
/// scanning continues. A `pub mod` returns the index of `mod`, so the scope
/// sees an inline module.
fn scan_at(tokens: &[Token], pos: usize, surface: &mut Surface) -> usize {
    if !is_ident(tokens.get(pos), "pub") || tokens.get(pos + 1) == Some(&Token::Punct('(')) {
        return pos + 1;
    }
    let item = skip_qualifiers(tokens, pos + 1);
    let Some(Token::Ident(keyword)) = tokens.get(item) else {
        // A tuple field such as `pub String` or `pub Vec<T>`.
        return collect(tokens, item, Stop::FieldEnd, surface);
    };
    match keyword.as_str() {
        "mod" => item,
        "macro_rules" | "crate" | "extern" => item + 1,
        "fn" => collect(tokens, item, Stop::Signature, surface),
        "struct" | "union" => collect(tokens, item, Stop::Body, surface),
        "type" => collect(tokens, item, Stop::Signature, surface),
        "const" | "static" => collect(tokens, item, Stop::Value, surface),
        "enum" => collect(tokens, item, Stop::ItemEnd, surface),
        "trait" => collect_trait(tokens, item, surface),
        _ if tokens.get(item + 1) == Some(&Token::Punct(':')) => {
            collect(tokens, item + 2, Stop::FieldEnd, surface)
        }
        _ => collect(tokens, item, Stop::FieldEnd, surface),
    }
}

/// Records the paths in `tokens[start..]` up to `stop` and returns where the
/// public part ends.
fn collect(tokens: &[Token], start: usize, stop: Stop, surface: &mut Surface) -> usize {
    let end = item_end(tokens, start, stop);
    surface.exposed.extend(paths_in(&tokens[start..end]));
    end
}

/// A trait's header and the signatures of its items, without test-only
/// items, the bodies of its provided methods, or the default values of its
/// constants and types.
fn collect_trait(tokens: &[Token], start: usize, surface: &mut Surface) -> usize {
    let end = item_end(tokens, start, Stop::ItemEnd);
    let mut filter = TraitFilter::default();
    let mut kept = Vec::new();
    let mut pos = start;
    while pos < end {
        let test_only_end = (filter.braces == 1).then(|| test_only_item_end(tokens, pos)).flatten();
        if let Some(member_end) = test_only_end {
            pos = member_end;
            continue;
        }
        if filter.keeps(tokens, pos) {
            kept.push(tokens[pos].clone());
        }
        pos += 1;
    }
    surface.exposed.extend(paths_in(&kept));
    end
}

/// Which tokens of a trait belong to its signatures.
#[derive(Default)]
struct TraitFilter {
    braces: usize,
    /// The nesting inside the current item of the trait body.
    nesting: Nesting,
    in_default: bool,
}

impl TraitFilter {
    /// Whether the token at `pos` is part of a signature: not inside a
    /// provided method's body, and not in an item's default from its `=` to
    /// its `;`. An `=` or `;` inside brackets, as in `Iterator<Item = T>` or
    /// `[u8; 1]`, is part of a type.
    fn keeps(&mut self, tokens: &[Token], pos: usize) -> bool {
        let in_signature = self.braces == 1 && !self.in_default;
        match tokens[pos] {
            Token::Punct('{') if self.nesting.is_flat() => self.braces += 1,
            Token::Punct('}') if self.nesting.is_flat() => self.braces -= 1,
            Token::Punct('=') if in_signature && self.nesting.is_flat() => self.in_default = true,
            Token::Punct(';') if self.ends_item() => {
                self.in_default = false;
                self.nesting = Nesting::default();
            }
            _ if in_signature => self.nesting.step(tokens, pos, true),
            _ => {}
        }
        self.braces <= 1 && !self.in_default && !matches!(tokens[pos], Token::Punct('{' | '}'))
    }

    /// Whether a `;` here ends an item of the trait body. Inside a default
    /// value brackets are not tracked, so any `;` at the item level ends it.
    const fn ends_item(&self) -> bool {
        self.braces == 1 && (self.in_default || self.nesting.is_flat())
    }
}

/// Skips `const`, `async`, `unsafe`, and `extern "ABI"` before `fn`. A
/// `const` counts only when the qualifiers after it end in `fn name`, so
/// `pub const r#unsafe: T` stays a constant.
fn skip_qualifiers(tokens: &[Token], pos: usize) -> usize {
    let after = skip_fn_qualifiers(tokens, pos + 1);
    let starts_fn =
        is_ident(tokens.get(after), "fn") && matches!(tokens.get(after + 1), Some(Token::Ident(_)));
    if is_ident(tokens.get(pos), "const") && starts_fn {
        return after;
    }
    skip_fn_qualifiers(tokens, pos)
}

/// Skips `async`, `unsafe`, and `extern` (the lexer drops the ABI string).
fn skip_fn_qualifiers(tokens: &[Token], mut pos: usize) -> usize {
    while ["async", "unsafe", "extern"]
        .iter()
        .any(|word| is_ident(tokens.get(pos), word))
    {
        pos += 1;
    }
    pos
}

/// Every `ident (:: ident)*` path in `tokens`, including single names.
fn paths_in(tokens: &[Token]) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut after_separator = false;
    for token in tokens {
        match token {
            Token::Ident(name) if after_separator || current.is_empty() => {
                current.push(name.clone());
                after_separator = false;
            }
            Token::Ident(name) => {
                paths.push(std::mem::replace(&mut current, vec![name.clone()]));
            }
            Token::PathSep => after_separator = true,
            Token::Punct(_) => {
                after_separator = false;
                if !current.is_empty() {
                    paths.push(std::mem::take(&mut current));
                }
            }
        }
    }
    if !current.is_empty() {
        paths.push(current);
    }
    paths
}

/// Replaces a first segment that an import binds with the imported path.
fn expand(path: &mut Vec<String>, imports: &[UsePath]) {
    let Some(first) = path.first() else { return };
    let import = imports
        .iter()
        .find(|import| import.binding.as_ref() == Some(first));
    if let Some(import) = import {
        path.splice(0..1, import.segments.iter().cloned());
    }
}

fn is_ident(token: Option<&Token>, word: &str) -> bool {
    matches!(token, Some(Token::Ident(name)) if name == word)
}
