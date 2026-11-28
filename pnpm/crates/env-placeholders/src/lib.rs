//! The `${...}` placeholders of a config value, and the backslashes that
//! escape them.
//!
//! A placeholder is `${`, a non-empty body holding no `$`, `{` or `}`, and
//! `}`. A `$` that starts none, such as one in an unfinished `${`, is text.
//! An odd number of backslashes right before the `$` escapes the
//! placeholder.
//!
//! Creusot proves, see `pnpm/VERIFICATION.md`, that no function here panics,
//! and that:
//!
//! - [`placeholders`] reports every placeholder of its text and nothing else,
//!   and cuts the text into its bytes in order, on character boundaries.
//! - [`placeholder_ranges`] returns the byte range of every placeholder that
//!   no backslash escapes and nothing else, in order, on character
//!   boundaries.

#[cfg(creusot)]
pub mod lemmas;
#[cfg(creusot)]
pub mod logic;

#[cfg(creusot)]
use creusot_std::prelude::{
    ensures,
    invariant,
    proof_assert,
    requires,
    snapshot,
};
#[cfg(creusot)]
use lemmas::no_placeholder_inside;
#[cfg(creusot)]
use logic::{
    escapes_before,
    is_body_byte,
    is_placeholder,
    last_end,
    previous_end,
    reported,
    scanned,
};
#[cfg(creusot)]
use pnpm_creusot_models::utf8::{
    ascii_byte,
    ends_are_boundaries,
    is_boundary,
};
use std::ops::Range;

// The bytes that mark a placeholder: `$`, `{` and `}`, and `\`, which
// escapes one.
const DOLLAR: u8 = b'$';
const OPEN: u8 = b'{';
const CLOSE: u8 = b'}';
const BACKSLASH: u8 = b'\\';

/// One placeholder of a text, escaped or not, and the text between it and
/// the placeholder before it.
#[derive(Debug, Clone, Copy)]
pub struct Placeholder<'text> {
    literal: &'text str,
    backslashes: usize,
    start: usize,
    end: usize,
    raw: &'text str,
    body: &'text str,
}

impl<'text> Placeholder<'text> {
    /// The text after the placeholder before this one, or from the start of
    /// the text, up to the backslashes written right before this one.
    #[must_use]
    pub fn literal(&self) -> &'text str {
        self.literal
    }

    /// How many backslashes are written right before the `$`. An odd count
    /// escapes the placeholder.
    #[must_use]
    pub fn backslashes(&self) -> usize {
        self.backslashes
    }

    /// The placeholder as written, from `$` to `}`.
    #[must_use]
    pub fn raw(&self) -> &'text str {
        self.raw
    }

    /// What the placeholder holds between `${` and `}`.
    #[must_use]
    pub fn body(&self) -> &'text str {
        self.body
    }
}

/// Every placeholder of `text` in order, escaped or not, and the text after
/// the last one.
#[cfg_attr(creusot, ensures(forall<index> 0 <= index && index < result.0@.len() ==>
    reported(text@, result.0@[index], previous_end(result.0@, index))))]
#[cfg_attr(creusot, ensures(forall<first, second> 0 <= first && first < second
    && second < result.0@.len() ==> result.0@[first].end@ <= result.0@[second].start@))]
#[cfg_attr(creusot, ensures(forall<start, end> is_placeholder(text@.to_bytes(), start, end) ==>
    exists<index> 0 <= index && index < result.0@.len()
        && result.0@[index].start@ == start && result.0@[index].end@ == end))]
#[cfg_attr(creusot, ensures(result.1@.to_bytes()
    == text@.to_bytes().subsequence(last_end(result.0@), text@.to_bytes().len())))]
#[must_use]
pub fn placeholders(text: &str) -> (Vec<Placeholder<'_>>, &str) {
    let bytes = text.as_bytes();
    let mut placeholders: Vec<Placeholder<'_>> = Vec::new();
    let mut literal_start = 0;
    let mut index = 0;
    #[cfg(creusot)]
    proof_assert! { ends_are_boundaries(text@); is_boundary(text@, 0) };
    #[cfg_attr(creusot, invariant(scanned(text@, placeholders@, literal_start@, index@)))]
    while index < bytes.len() {
        match placeholder_end(bytes, index) {
            Some(end) => {
                record(text, &mut placeholders, literal_start, index, end);
                literal_start = end;
                index = end;
            }
            None => index += 1,
        }
    }
    (placeholders, &text[literal_start..])
}

/// Append the placeholder from byte `start` to byte `end` of `text`, with
/// the text since `literal_start` before it.
#[cfg_attr(creusot, requires(scanned(text@, placeholders@, literal_start@, start@)))]
#[cfg_attr(creusot, requires(is_placeholder(text@.to_bytes(), start@, end@)))]
#[cfg_attr(creusot, ensures(scanned(text@, (^placeholders)@, end@, end@)))]
fn record<'text>(
    text: &'text str,
    placeholders: &mut Vec<Placeholder<'text>>,
    literal_start: usize,
    start: usize,
    end: usize,
) {
    let placeholder = cut_placeholder(text, literal_start, start, end);
    #[cfg(creusot)]
    proof_assert! {
        no_placeholder_inside(text@.to_bytes(), start@, end@);
        forall<other_start, other_end> is_placeholder(text@.to_bytes(), other_start, other_end)
            && start@ <= other_start && other_start < end@ ==> other_start == start@ && other_end == end@
    };
    #[cfg(creusot)]
    let previous = snapshot! { placeholders@ };
    placeholders.push(placeholder);
    #[cfg(creusot)]
    proof_assert! {
        placeholders@.len() == previous.len() + 1
            && forall<index> 0 <= index && index < previous.len() ==> placeholders@[index] == previous[index]
    };
    #[cfg(creusot)]
    proof_assert! { previous_end(placeholders@, placeholders@.len() - 1) == literal_start@ };
    #[cfg(creusot)]
    proof_assert! { reported(text@, placeholders@[placeholders@.len() - 1], literal_start@) };
    #[cfg(creusot)]
    proof_assert! {
        placeholders@[placeholders@.len() - 1].start == start
            && placeholders@[placeholders@.len() - 1].end == end
    };
    #[cfg(creusot)]
    proof_assert! {
        forall<other_start, other_end> is_placeholder(text@.to_bytes(), other_start, other_end)
            && other_start < start@ ==>
            exists<index> 0 <= index && index < placeholders@.len() - 1
                && placeholders@[index].start@ == other_start
                && placeholders@[index].end@ == other_end
    };
}

/// The placeholder from byte `start` to byte `end` of `text`, with the text
/// since `literal_start` before it.
#[cfg_attr(creusot, requires(literal_start@ <= start@))]
#[cfg_attr(creusot, requires(literal_start@ == 0 || text@.to_bytes()[literal_start@ - 1] == CLOSE))]
#[cfg_attr(creusot, requires(is_boundary(text@, literal_start@)))]
#[cfg_attr(creusot, requires(is_placeholder(text@.to_bytes(), start@, end@)))]
#[cfg_attr(creusot, ensures(reported(text@, result, literal_start@)))]
#[cfg_attr(creusot, ensures(result.start == start && result.end == end))]
fn cut_placeholder(text: &str, literal_start: usize, start: usize, end: usize) -> Placeholder<'_> {
    let backslashes = backslashes_before(text.as_bytes(), literal_start, start);
    #[cfg(creusot)]
    proof_assert! { ascii_byte(text@, start@ - backslashes@); is_boundary(text@, start@ - backslashes@) };
    #[cfg(creusot)]
    proof_assert! {
        ascii_byte(text@, start@);
        ascii_byte(text@, start@ + 1);
        is_boundary(text@, start@) && is_boundary(text@, start@ + 2)
    };
    #[cfg(creusot)]
    proof_assert! { ascii_byte(text@, end@ - 1); is_boundary(text@, end@ - 1) && is_boundary(text@, end@) };
    Placeholder {
        literal: &text[literal_start..start - backslashes],
        backslashes,
        start,
        end,
        raw: &text[start..end],
        body: &text[start + 2..end - 1],
    }
}

/// The byte ranges of the placeholders of `text` that no backslash escapes,
/// in order.
///
/// A caller that resolves placeholders itself, rather than taking the
/// substituted string from `pnpm-env-replace`, reads them from here. Both
/// read [`placeholders`], so they agree on what a placeholder is: an
/// unfinished `${`, for one, is text.
#[cfg_attr(creusot, ensures(forall<index> 0 <= index && index < result@.len() ==>
    is_placeholder(text@.to_bytes(), result@[index].start@, result@[index].end@)
        && is_boundary(text@, result@[index].start@) && is_boundary(text@, result@[index].end@)
        && escapes_before(text@.to_bytes(), result@[index].start@) % 2 == 0))]
#[cfg_attr(creusot, ensures(forall<first, second> 0 <= first && first < second
    && second < result@.len() ==> result@[first].end@ <= result@[second].start@))]
#[cfg_attr(creusot, ensures(forall<start, end> is_placeholder(text@.to_bytes(), start, end)
    && escapes_before(text@.to_bytes(), start) % 2 == 0 ==>
    exists<index> 0 <= index && index < result@.len()
        && result@[index].start@ == start && result@[index].end@ == end))]
#[must_use]
pub fn placeholder_ranges(text: &str) -> Vec<Range<usize>> {
    let (placeholders, _) = placeholders(text);
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut index = 0;
    #[cfg_attr(creusot, invariant(index@ <= placeholders@.len()))]
    #[cfg_attr(creusot, invariant(forall<range> 0 <= range && range < ranges@.len() ==>
        exists<placeholder> 0 <= placeholder && placeholder < index@
            && placeholders@[placeholder].backslashes@ % 2 == 0
            && ranges@[range].start == placeholders@[placeholder].start
            && ranges@[range].end == placeholders@[placeholder].end))]
    #[cfg_attr(creusot, invariant(forall<range, placeholder> 0 <= range && range < ranges@.len()
        && index@ <= placeholder && placeholder < placeholders@.len() ==>
        ranges@[range].end@ <= placeholders@[placeholder].start@))]
    #[cfg_attr(creusot, invariant(forall<first, second> 0 <= first && first < second
        && second < ranges@.len() ==> ranges@[first].end@ <= ranges@[second].start@))]
    #[cfg_attr(creusot, invariant(forall<placeholder> 0 <= placeholder && placeholder < index@
        && placeholders@[placeholder].backslashes@ % 2 == 0 ==>
        exists<range> 0 <= range && range < ranges@.len()
            && ranges@[range].start == placeholders@[placeholder].start
            && ranges@[range].end == placeholders@[placeholder].end))]
    while index < placeholders.len() {
        let placeholder = &placeholders[index];
        if placeholder.backslashes % 2 == 0 {
            ranges.push(placeholder.start..placeholder.end);
        }
        index += 1;
    }
    ranges
}

/// Where the placeholder starting at byte `start` of `bytes` ends, one past
/// its `}`, if one starts there.
#[cfg_attr(creusot, requires(start@ < bytes@.len()))]
#[cfg_attr(creusot, ensures(match result {
    Some(end) => is_placeholder(bytes@, start@, end@),
    None => forall<end> !is_placeholder(bytes@, start@, end),
}))]
fn placeholder_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes[start] != DOLLAR || start + 1 == bytes.len() || bytes[start + 1] != OPEN {
        return None;
    }
    let body_start = start + 2;
    let mut cursor = body_start;
    #[cfg_attr(creusot, invariant(body_start@ <= cursor@ && cursor@ <= bytes@.len()))]
    #[cfg_attr(creusot, invariant(forall<index> body_start@ <= index && index < cursor@ ==>
        is_body_byte(bytes@[index])))]
    while cursor < bytes.len() {
        match bytes[cursor] {
            CLOSE if cursor > body_start => return Some(cursor + 1),
            DOLLAR | OPEN | CLOSE => return None,
            _ => cursor += 1,
        }
    }
    None
}

/// How many backslashes come right before byte `index` of `bytes`, counting
/// back no further than `from`.
#[cfg_attr(creusot, requires(from@ <= index@ && index@ <= bytes@.len()))]
#[cfg_attr(creusot, requires(from@ == 0 || bytes@[from@ - 1] != BACKSLASH))]
#[cfg_attr(creusot, ensures(result@ <= index@ - from@))]
#[cfg_attr(creusot, ensures(result@ == escapes_before(bytes@, index@)))]
#[cfg_attr(creusot, ensures(forall<escape> index@ - result@ <= escape && escape < index@ ==>
    bytes@[escape] == BACKSLASH))]
fn backslashes_before(bytes: &[u8], from: usize, index: usize) -> usize {
    let mut backslashes = 0;
    #[cfg_attr(creusot, invariant(backslashes@ <= index@ - from@))]
    #[cfg_attr(creusot, invariant(forall<escape> index@ - backslashes@ <= escape && escape < index@ ==>
        bytes@[escape] == BACKSLASH))]
    #[cfg_attr(creusot, invariant(escapes_before(bytes@, index@)
        == backslashes@ + escapes_before(bytes@, index@ - backslashes@)))]
    while backslashes < index - from && bytes[index - 1 - backslashes] == BACKSLASH {
        backslashes += 1;
    }
    backslashes
}

#[cfg(test)]
mod tests;
