//! The headers of items in the token stream: where an item's name or self
//! type is, past its visibility and qualifiers, and the paths a span names.

use crate::lexer::Token;

/// An item at a scope's top level that owns the signatures inside it.
pub enum ItemHeader {
    Named {
        name: String,
        is_public: bool,
        namespace: Namespace,
    },
    /// An `impl` block, owned by its self type.
    Impl {
        self_type: Vec<String>,
    },
}

/// The namespace a named item's name binds in. A type and a function or
/// constant of the same name can coexist in one module.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Type,
    Value,
}

/// The keywords of the items that [`ItemHeader::Named`] reads.
const NAMED_ITEM_KEYWORDS: [&str; 8] =
    ["fn", "struct", "enum", "union", "trait", "type", "const", "static"];

/// The header of the item that starts at `pos`, if one does. An item starts
/// only after the end of another item, an attribute, or the `{` of a foreign
/// block, so `impl Trait` in a type and `const N` in generics are not items.
pub fn item_header(tokens: &[Token], pos: usize) -> Option<ItemHeader> {
    let starts_item = pos
        .checked_sub(1)
        .is_none_or(|before| matches!(tokens[before], Token::Punct(';' | '{' | '}' | ']')));
    if !starts_item {
        return None;
    }
    let after_visibility = skip_visibility(tokens, pos);
    let item = skip_qualifiers(tokens, after_visibility);
    let Some(Token::Ident(keyword)) = tokens.get(item) else { return None };
    if keyword == "impl" {
        return Some(ItemHeader::Impl { self_type: impl_self_type(tokens, item + 1) });
    }
    if !NAMED_ITEM_KEYWORDS.contains(&keyword.as_str()) {
        return None;
    }
    let is_static_mut = keyword == "static"
        && is_ident(tokens.get(item + 1), "mut")
        && matches!(tokens.get(item + 2), Some(Token::Ident(_)));
    let name_at = if is_static_mut { item + 2 } else { item + 1 };
    let Some(Token::Ident(name)) = tokens.get(name_at) else { return None };
    let is_public = after_visibility == pos + 1;
    let namespace = if ["fn", "const", "static"].contains(&keyword.as_str()) {
        Namespace::Value
    } else {
        Namespace::Type
    };
    Some(ItemHeader::Named { name: name.clone(), is_public, namespace })
}

/// The path of the self type of the `impl` whose generics or type start at
/// `start`: the first path after `for`, or after the generics when there is
/// no `for`.
fn impl_self_type(tokens: &[Token], start: usize) -> Vec<String> {
    let mut type_start = skip_generics(tokens, start);
    let mut angles = Angles::default();
    let mut end = type_start;
    while let Some(token) = tokens.get(end) {
        if angles.is_flat() {
            match token {
                Token::Punct('{' | ';') => break,
                Token::Ident(word) if word == "where" => break,
                Token::Ident(word) if word == "for" => type_start = end + 1,
                _ => {}
            }
        }
        angles.step(tokens, end);
        end += 1;
    }
    paths_in(&tokens[type_start..end])
        .into_iter()
        .find(|path| !matches!(path.as_slice(), [word] if word == "dyn" || word == "mut"))
        .unwrap_or_default()
}

/// The index after the generics `<…>` at `pos`, or `pos` when there are none.
fn skip_generics(tokens: &[Token], pos: usize) -> usize {
    if tokens.get(pos) != Some(&Token::Punct('<')) {
        return pos;
    }
    let mut angles = Angles::default();
    let mut end = pos;
    while end < tokens.len() {
        angles.step(tokens, end);
        end += 1;
        if angles.is_flat() {
            break;
        }
    }
    end
}

/// The depth of `<…>` brackets. The `>` of `->` closes none.
#[derive(Default)]
struct Angles(usize);

impl Angles {
    const fn is_flat(&self) -> bool {
        self.0 == 0
    }

    fn step(&mut self, tokens: &[Token], pos: usize) {
        let after_dash = pos
            .checked_sub(1)
            .is_some_and(|before| tokens[before] == Token::Punct('-'));
        match tokens[pos] {
            Token::Punct('<') => self.0 += 1,
            Token::Punct('>') if !after_dash => self.0 = self.0.saturating_sub(1),
            _ => {}
        }
    }
}

/// The index after `pub` or `pub(…)` at `pos`, or `pos` when there is none.
pub fn skip_visibility(tokens: &[Token], pos: usize) -> usize {
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

/// Skips `const`, `async`, `unsafe`, and `extern "ABI"` before `fn`. A
/// `const` counts only when the qualifiers after it end in `fn name`, so
/// `pub const r#unsafe: T` stays a constant.
pub fn skip_qualifiers(tokens: &[Token], pos: usize) -> usize {
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
pub fn paths_in(tokens: &[Token]) -> Vec<Vec<String>> {
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

pub fn is_ident(token: Option<&Token>, word: &str) -> bool {
    matches!(token, Some(Token::Ident(name)) if name == word)
}
