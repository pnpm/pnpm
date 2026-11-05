//! The paths a file's public items name in their signatures, read from the
//! token stream. Visibility is taken from the `pub` keyword alone: a `pub`
//! item inside a private module counts as public.

use crate::{
    cfg,
    lexer::Token,
    use_tree::{self, UsePath},
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Surface {
    /// Paths named in public signatures, with a first segment that a `use`
    /// in the same file imports replaced by the imported path.
    pub exposed: Vec<Vec<String>>,
    /// The paths of `pub use` items.
    pub reexported: Vec<UsePath>,
    /// `extern crate name as alias;` items, which bind `alias` in the whole
    /// crate.
    pub crate_aliases: Vec<UsePath>,
}

/// The surface of one file. Test-only items, including their imports, are
/// left out.
pub fn scan(tokens: &[Token]) -> Surface {
    let mut surface = Surface::default();
    let mut imports = Vec::new();
    let mut pos = 0;
    while pos < tokens.len() {
        if let Some(alias) = extern_crate_alias(tokens, pos) {
            surface.crate_aliases.push(alias);
        }
        pos = if let Some(end) = test_only_item_end(tokens, pos) {
            end
        } else if let Some(item) = use_item(tokens, pos) {
            item.record(&mut surface, &mut imports)
        } else {
            scan_at(tokens, pos, &mut surface).max(pos + 1)
        };
    }
    for path in &mut surface.exposed {
        expand(path, &imports);
    }
    surface
}

/// The keywords an item starts with, after its visibility and qualifiers.
const ITEM_KEYWORDS: [&str; 10] =
    ["use", "mod", "fn", "struct", "enum", "trait", "impl", "const", "static", "type"];

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

impl UseItem {
    /// Adds the paths to the file's imports, and to its re-exports for a
    /// `pub use`. Returns the index after the item.
    fn record(self, surface: &mut Surface, imports: &mut Vec<UsePath>) -> usize {
        if self.is_public {
            surface.reexported.extend(self.paths.iter().cloned());
        }
        imports.extend(self.paths);
        self.next
    }
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
/// scanning continues.
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
        "mod" | "macro_rules" | "crate" | "extern" => item + 1,
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

/// Where the public part of an item ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// Before a body or tuple fields: `{`, `(`, or `;` outside brackets.
    Body,
    /// At `{` or `;` outside brackets: a function's body, or the end of a
    /// type alias or a body-less trait method.
    Signature,
    /// At `=` or `;` outside brackets.
    Value,
    /// At `,` or a closing bracket outside the field's own brackets.
    FieldEnd,
    /// After the item's `{ … }` body, or at `;` before one.
    ItemEnd,
    /// Like `ItemEnd`, and also at `,` or a closing bracket, so a field
    /// ends where it does.
    Extent,
}

/// Records the paths in `tokens[start..]` up to `stop` and returns where the
/// public part ends.
fn collect(tokens: &[Token], start: usize, stop: Stop, surface: &mut Surface) -> usize {
    let end = item_end(tokens, start, stop);
    surface.exposed.extend(paths_in(&tokens[start..end]));
    end
}

fn item_end(tokens: &[Token], start: usize, stop: Stop) -> usize {
    let end = boundary(tokens, start, stop);
    let opens_body =
        matches!(stop, Stop::ItemEnd | Stop::Extent) && tokens.get(end) == Some(&Token::Punct('{'));
    if opens_body { closing_brace(tokens, end) } else { end }
}

/// The first token from `start` that ends the item under `stop` outside
/// brackets, or the bracket that closes the one the item is in.
fn boundary(tokens: &[Token], start: usize, stop: Stop) -> usize {
    let mut depth = 0usize;
    for (pos, token) in tokens.iter().enumerate().skip(start) {
        let Token::Punct(punct) = token else { continue };
        if depth == 0 && (ends_at(*punct, stop) || matches!(punct, ')' | ']' | '}')) {
            return pos;
        }
        if stop.tracks_angles() || !matches!(punct, '<' | '>') {
            depth = nested_depth(depth, tokens, pos);
        }
    }
    tokens.len()
}

/// The bracket depth after the punctuation at `pos`, counting `<` and `>`
/// but not the `>` of an arrow.
fn nested_depth(depth: usize, tokens: &[Token], pos: usize) -> usize {
    match tokens[pos] {
        Token::Punct('(' | '[' | '{' | '<') => depth + 1,
        Token::Punct('>') if is_arrow(tokens, pos) => depth,
        Token::Punct(')' | ']' | '}' | '>') => depth.saturating_sub(1),
        _ => depth,
    }
}

impl Stop {
    /// Whether `<` and `>` count as brackets. They must where a `,` or `(`
    /// inside generics would end the scan early. Elsewhere they are left
    /// out, so a comparison or shift in a value, as in `1 << 3`, cannot
    /// hide the item's end.
    fn tracks_angles(self) -> bool {
        matches!(self, Self::Body | Self::FieldEnd | Self::Extent)
    }
}

fn ends_at(punct: char, stop: Stop) -> bool {
    match stop {
        Stop::Body => matches!(punct, '{' | '(' | ';'),
        Stop::Signature => matches!(punct, '{' | ';'),
        Stop::Value => matches!(punct, '=' | ';'),
        Stop::FieldEnd => punct == ',',
        Stop::ItemEnd => matches!(punct, '{' | ';'),
        Stop::Extent => matches!(punct, '{' | ';' | ','),
    }
}

/// Whether the `>` at `pos` is the second half of `->` or `=>`.
fn is_arrow(tokens: &[Token], pos: usize) -> bool {
    pos > 0 && matches!(tokens[pos - 1], Token::Punct('-' | '='))
}

/// The index after the `}` that closes the `{` at `open`.
fn closing_brace(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0usize;
    for (offset, token) in tokens[open..].iter().enumerate() {
        match token {
            Token::Punct('{') => depth += 1,
            Token::Punct('}') if depth == 1 => return open + offset + 1,
            Token::Punct('}') => depth -= 1,
            _ => {}
        }
    }
    tokens.len()
}

/// A trait's header and the signatures of its items, without the bodies of
/// its provided methods or the default values of its constants and types.
fn collect_trait(tokens: &[Token], start: usize, surface: &mut Surface) -> usize {
    let end = item_end(tokens, start, Stop::ItemEnd);
    let mut filter = TraitFilter::default();
    let kept = (start..end)
        .filter(|pos| filter.keeps(tokens, *pos))
        .map(|pos| tokens[pos].clone())
        .collect::<Vec<_>>();
    surface.exposed.extend(paths_in(&kept));
    end
}

/// Which tokens of a trait belong to its signatures.
#[derive(Default)]
struct TraitFilter {
    braces: usize,
    /// The depth of `(`, `[`, and `<` inside the trait body.
    brackets: usize,
    in_default: bool,
}

impl TraitFilter {
    /// Whether the token at `pos` is part of a signature: not inside a
    /// provided method's body, and not in an item's default from its `=` to
    /// its `;`. An `=` inside brackets, as in `Iterator<Item = T>`, is part of
    /// a bound.
    fn keeps(&mut self, tokens: &[Token], pos: usize) -> bool {
        let in_signature = self.braces == 1 && !self.in_default;
        match tokens[pos] {
            Token::Punct('{') => self.braces += 1,
            Token::Punct('}') => self.braces -= 1,
            Token::Punct('=') if in_signature && self.brackets == 0 => self.in_default = true,
            Token::Punct(';') if self.braces == 1 => {
                self.in_default = false;
                self.brackets = 0;
            }
            Token::Punct('(' | '[' | '<' | ')' | ']' | '>') if in_signature => {
                self.brackets = nested_depth(self.brackets, tokens, pos);
            }
            _ => {}
        }
        self.braces <= 1 && !self.in_default && !matches!(tokens[pos], Token::Punct('{' | '}'))
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
