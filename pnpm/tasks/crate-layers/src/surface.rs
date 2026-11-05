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

pub fn scan(tokens: &[Token], imports: &[UsePath]) -> Surface {
    let mut surface = Surface::default();
    let mut pos = 0;
    while pos < tokens.len() {
        if let Some(alias) = extern_crate_alias(tokens, pos) {
            surface.crate_aliases.push(alias);
        }
        if let Some(end) = test_only_inline_module_end(tokens, pos) {
            pos = end;
            continue;
        }
        pos = scan_at(tokens, pos, &mut surface).max(pos + 1);
    }
    for path in &mut surface.exposed {
        expand(path, imports);
    }
    surface
}

/// The index after a test-only inline module `mod name { … }` that starts at
/// `pos`.
fn test_only_inline_module_end(tokens: &[Token], pos: usize) -> Option<usize> {
    let is_inline_module = is_ident(tokens.get(pos), "mod")
        && matches!(tokens.get(pos + 1), Some(Token::Ident(_)))
        && tokens.get(pos + 2) == Some(&Token::Punct('{'));
    (is_inline_module && cfg::is_test_only(tokens, pos)).then(|| closing_brace(tokens, pos + 2))
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
        "use" => {
            let (paths, next) = use_tree::parse(tokens, item + 1);
            surface.reexported.extend(paths);
            next
        }
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
    let opens_body = stop == Stop::ItemEnd && tokens.get(end) == Some(&Token::Punct('{'));
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
        depth = nested_depth(depth, tokens, pos);
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

fn ends_at(punct: char, stop: Stop) -> bool {
    match stop {
        Stop::Body => matches!(punct, '{' | '(' | ';'),
        Stop::Signature => matches!(punct, '{' | ';'),
        Stop::Value => matches!(punct, '=' | ';'),
        Stop::FieldEnd => punct == ',',
        Stop::ItemEnd => matches!(punct, '{' | ';'),
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
    let mut depth = 0usize;
    let mut in_default = false;
    let mut kept = Vec::new();
    for token in &tokens[start..end] {
        match token {
            Token::Punct('{') => depth += 1,
            Token::Punct('}') => depth -= 1,
            Token::Punct('=') if depth == 1 => in_default = true,
            Token::Punct(';') if depth == 1 => in_default = false,
            _ if depth <= 1 && !in_default => kept.push(token.clone()),
            _ => {}
        }
    }
    surface.exposed.extend(paths_in(&kept));
    end
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
