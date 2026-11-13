//! The paths a file's public items name in their signatures, read from the
//! token stream. Each path records the item that owns it, so
//! [`crate::visibility`] can keep only the items reachable from outside the
//! crate.

use crate::{
    cfg,
    extent::{Nesting, Stop, closing_brace, item_end},
    item::{
        ItemHeader, Namespace, is_ident, item_header, paths_in, skip_qualifiers, skip_visibility,
    },
    lexer::Token,
    use_tree::{self, UsePath},
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Surface {
    /// Paths named in public signatures.
    pub exposed: Vec<Exposure>,
    /// The names of the `pub` items at the scope's top level.
    pub public_items: Vec<String>,
    /// The types declared at the scope's top level, `pub` or not.
    pub types: Vec<TypeDeclaration>,
    /// The paths of `pub use` items.
    pub reexported: Vec<UsePath>,
    /// `extern crate name as alias;` items. An alias binds in its own module,
    /// and in the whole crate when the crate root declares it.
    pub crate_aliases: Vec<UsePath>,
    /// The inline modules `mod name { … }`, each a scope of its own.
    pub inline_modules: Vec<InlineModule>,
}

/// A path named in a public signature, with a first segment that a `use` in
/// the same scope imports replaced by the imported path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exposure {
    pub path: Vec<String>,
    /// The top-level item the signature belongs to, written as a path from
    /// the scope: the item's own name, or the self type of an `impl`. Empty
    /// when the signature is outside any item.
    pub owner: Vec<String>,
    /// The namespace the owner's name binds in: a type for an `impl`.
    pub owner_namespace: Namespace,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TypeDeclaration {
    pub name: String,
    pub is_public: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InlineModule {
    pub name: String,
    pub is_public: bool,
    pub surface: Surface,
}

/// The surface of one file. Test-only items, including their imports, are
/// left out.
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
    /// The top-level item the scan is in, and the namespace its name binds
    /// in.
    owner: (Vec<String>, Namespace),
    /// The depth of `{` blocks: a `use` inside a function body or another
    /// block is not one of the scope's imports.
    braces: usize,
    /// The `}` of the foreign block `extern "ABI" { … }` the scan is in. Its
    /// items belong to the scope, so its braces do not count.
    foreign_block_close: Option<usize>,
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
        if self.braces == 0 {
            if let Some(next) = self.foreign_block(pos) {
                return next;
            }
            self.enter_item(pos);
        }
        match tokens[pos] {
            Token::Punct('{') => self.braces += 1,
            Token::Punct('}') => self.braces = self.braces.saturating_sub(1),
            _ => {}
        }
        if let Some(item) = use_item(tokens, pos) {
            return self.record_use(item);
        }
        let mut exposed = Vec::new();
        let next = scan_at(tokens, pos, &mut exposed);
        let owned = exposed
            .into_iter()
            .map(|path| Exposure {
                path,
                owner: self.owner.0.clone(),
                owner_namespace: self.owner.1,
            });
        self.surface.exposed.extend(owned);
        next
    }

    /// Steps over the `extern {` that opens a foreign block, or the `}` that
    /// closes the one the scan is in.
    fn foreign_block(&mut self, pos: usize) -> Option<usize> {
        let tokens = self.tokens;
        if self.foreign_block_close == Some(pos) {
            self.foreign_block_close = None;
            return Some(pos + 1);
        }
        let opens =
            is_ident(tokens.get(pos), "extern") && tokens.get(pos + 1) == Some(&Token::Punct('{'));
        if !opens {
            return None;
        }
        self.foreign_block_close = Some(closing_brace(tokens, pos + 1) - 1);
        Some(pos + 2)
    }

    /// Makes the item that starts at `pos`, if one does, the owner of the
    /// signatures that follow, and records its name when it is `pub` or a
    /// type.
    fn enter_item(&mut self, pos: usize) {
        let Some(header) = item_header(self.tokens, pos) else { return };
        if let ItemHeader::Named { name, is_public, namespace } = &header {
            if *is_public {
                self.surface.public_items.push(name.clone());
            }
            if *namespace == Namespace::Type {
                let declaration = TypeDeclaration { name: name.clone(), is_public: *is_public };
                self.surface.types.push(declaration);
            }
        }
        self.owner = match header {
            ItemHeader::Named { name, namespace, .. } => (vec![name], namespace),
            ItemHeader::Impl { self_type } => (self_type, Namespace::Type),
        };
    }

    /// Scans the inline module `mod name { … }` at `pos` as its own scope.
    fn inline_module(&mut self, pos: usize) -> Option<usize> {
        let tokens = self.tokens;
        let Some(Token::Ident(name)) = tokens.get(pos + 1) else { return None };
        let opens =
            is_ident(tokens.get(pos), "mod") && tokens.get(pos + 2) == Some(&Token::Punct('{'));
        if !opens {
            return None;
        }
        let is_public = pos > 0 && is_ident(tokens.get(pos - 1), "pub");
        let end = closing_brace(tokens, pos + 2);
        let body = tokens
            .get(pos + 3..end.saturating_sub(1))
            .unwrap_or_default();
        let surface = scan(body);
        self.surface.inline_modules.push(InlineModule { name: name.clone(), is_public, surface });
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
        for exposure in &mut self.surface.exposed {
            expand(&mut exposure.path, &self.imports);
            expand(&mut exposure.owner, &self.imports);
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
fn scan_at(tokens: &[Token], pos: usize, exposed: &mut Vec<Vec<String>>) -> usize {
    if !is_ident(tokens.get(pos), "pub") || tokens.get(pos + 1) == Some(&Token::Punct('(')) {
        return pos + 1;
    }
    let item = skip_qualifiers(tokens, pos + 1);
    let Some(Token::Ident(keyword)) = tokens.get(item) else {
        // A tuple field such as `pub String` or `pub Vec<T>`.
        return collect(tokens, item, Stop::FieldEnd, exposed);
    };
    match keyword.as_str() {
        "mod" => item,
        "macro_rules" | "crate" | "extern" => item + 1,
        "fn" => collect(tokens, item, Stop::Signature, exposed),
        "struct" | "union" => collect(tokens, item, Stop::Body, exposed),
        "type" => collect(tokens, item, Stop::Signature, exposed),
        "const" | "static" => collect(tokens, item, Stop::Value, exposed),
        "enum" => collect(tokens, item, Stop::ItemEnd, exposed),
        "trait" => collect_trait(tokens, item, exposed),
        _ if tokens.get(item + 1) == Some(&Token::Punct(':')) => {
            collect(tokens, item + 2, Stop::FieldEnd, exposed)
        }
        _ => collect(tokens, item, Stop::FieldEnd, exposed),
    }
}

/// Records the paths in `tokens[start..]` up to `stop` and returns where the
/// public part ends.
fn collect(tokens: &[Token], start: usize, stop: Stop, exposed: &mut Vec<Vec<String>>) -> usize {
    let end = item_end(tokens, start, stop);
    exposed.extend(paths_in(&tokens[start..end]));
    end
}

/// A trait's header and the signatures of its items, without test-only
/// items, the bodies of its provided methods, or the default values of its
/// constants and types.
fn collect_trait(tokens: &[Token], start: usize, exposed: &mut Vec<Vec<String>>) -> usize {
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
    exposed.extend(paths_in(&kept));
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
