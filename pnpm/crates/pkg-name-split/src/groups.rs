//! The parenthesized groups that end a dep path, such as its peer suffix
//! `(react@18.2.0)` and its `(patch_hash=...)`, read right to left. A
//! locator such as a `file:` path can hold parentheses of its own, so the
//! groups are found from the end.

#[cfg(creusot)]
pub mod spec;

#[cfg(creusot)]
use creusot_std::prelude::{
    Invariant,
    ensures,
    invariant,
    logic,
    proof_assert,
    requires,
};
#[cfg(creusot)]
use pnpm_creusot_models::{
    str_model::str_size_bound,
    utf8::{
        ascii_byte,
        ends_are_boundaries,
        is_boundary,
    },
};
#[cfg(creusot)]
use spec::{
    excess,
    is_group,
    is_paren,
    tiles,
};

// The bytes that open and close a group.
const OPEN: u8 = b'(';
const CLOSE: u8 = b')';

/// A parenthesized group of a text: a `(`, then text in which every `(` is
/// closed, then the `)` that closes the first `(`.
#[derive(Debug, Clone, Copy)]
pub struct Group<'text> {
    text: &'text str,
    start: usize,
    end: usize,
}

#[cfg(creusot)]
impl Invariant for Group<'_> {
    #[logic(open(self), inline)]
    fn invariant(self) -> bool {
        pearlite! {
            is_group(self.text@.to_bytes(), self.start@, self.end@)
                && is_boundary(self.text@, self.start@)
                && is_boundary(self.text@, self.end@)
        }
    }
}

impl<'text> Group<'text> {
    /// The group as written, from `(` to `)`.
    #[cfg_attr(creusot, ensures(result@.to_bytes()
        == self.text@.to_bytes().subsequence(self.start@, self.end@)))]
    #[must_use]
    pub fn as_str(self) -> &'text str {
        &self.text[self.start..self.end]
    }

    /// What the group holds between its `(` and `)`.
    #[cfg_attr(creusot, ensures(result@.to_bytes()
        == self.text@.to_bytes().subsequence(self.start@ + 1, self.end@ - 1)))]
    #[must_use]
    pub fn body(self) -> &'text str {
        #[cfg(creusot)]
        proof_assert! {
            ascii_byte(self.text@, self.start@);
            ascii_byte(self.text@, self.end@ - 1);
            is_boundary(self.text@, self.start@ + 1) && is_boundary(self.text@, self.end@ - 1)
        };
        &self.text[self.start + 1..self.end - 1]
    }
}

/// The run of back-to-back groups that ends `text`, and the text in front of
/// it. `None` when one of those groups has no `(`.
#[cfg_attr(creusot, ensures(match result {
    Some((front, groups)) => front@.to_bytes()
        == text@.to_bytes().subsequence(0, front@.to_bytes().len())
        && tiles(text@, groups@, front@.to_bytes().len())
        && (front@.to_bytes().len() == 0
            || text@.to_bytes()[front@.to_bytes().len() - 1] != CLOSE),
    None => exists<end> 0 < end && end <= text@.to_bytes().len()
        && text@.to_bytes()[end - 1] == CLOSE
        && forall<start> !is_group(text@.to_bytes(), start, end),
}))]
#[must_use]
pub fn split_trailing_groups(text: &str) -> Option<(&str, Vec<Group<'_>>)> {
    let bytes = text.as_bytes();
    let mut groups: Vec<Group<'_>> = Vec::new();
    let mut end = text.len();
    #[cfg(creusot)]
    proof_assert! { ends_are_boundaries(text@); is_boundary(text@, end@) };
    #[cfg_attr(creusot, invariant(end@ <= bytes@.len() && is_boundary(text@, end@)))]
    #[cfg_attr(creusot, invariant(tiles(text@, groups@, end@)))]
    while end > 0 && bytes[end - 1] == CLOSE {
        let Some(start) = matching_open(bytes, end - 1) else {
            #[cfg(creusot)]
            proof_assert! {
                0 < end@ && end@ <= text@.to_bytes().len()
                    && text@.to_bytes()[end@ - 1] == CLOSE
                    && forall<start> !is_group(text@.to_bytes(), start, end@)
            };
            return None;
        };
        #[cfg(creusot)]
        proof_assert! { ascii_byte(text@, start@); is_boundary(text@, start@) };
        // Inserted at the front rather than pushed and reversed: `creusot-std`
        // specifies `Vec::insert` and no `reverse`, and a dep path ends in a
        // handful of groups.
        groups.insert(0, Group { text, start, end });
        end = start;
    }
    Some((&text[..end], groups))
}

/// The index of the `(` that the `)` at `close` closes.
#[cfg_attr(creusot, requires(close@ < bytes@.len() && bytes@[close@] == CLOSE))]
#[cfg_attr(creusot, ensures(match result {
    Some(open) => is_group(bytes@, open@, close@ + 1),
    None => forall<open> !is_group(bytes@, open, close@ + 1),
}))]
fn matching_open(bytes: &[u8], close: usize) -> Option<usize> {
    let mut depth: usize = 1;
    let mut index = close;
    #[cfg_attr(creusot, invariant(index@ <= close@ && depth@ == excess(bytes@, index@, close@ + 1)))]
    #[cfg_attr(creusot, invariant(depth@ <= close@ + 1 - index@))]
    #[cfg_attr(creusot, invariant(forall<later> index@ <= later && later <= close@ ==>
        excess(bytes@, later, close@ + 1) > 0))]
    while index > 0 {
        index -= 1;
        match bytes[index] {
            CLOSE => depth += 1,
            OPEN => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Where the parenthesized suffix that ends `dep_path` starts: after the
/// last byte that is not a parenthesis and has as many `)` as `(` after it.
/// `None` when `dep_path` does not end with `)` or has no such byte.
#[cfg_attr(creusot, ensures(match result {
    Some(start) => 0 < start@ && start@ < dep_path@.to_bytes().len()
        && is_boundary(dep_path@, start@)
        && dep_path@.to_bytes()[dep_path@.to_bytes().len() - 1] == CLOSE
        && !is_paren(dep_path@.to_bytes()[start@ - 1])
        && excess(dep_path@.to_bytes(), start@, dep_path@.to_bytes().len()) == 0
        && forall<index> start@ <= index && index < dep_path@.to_bytes().len()
            && !is_paren(dep_path@.to_bytes()[index]) ==>
            excess(dep_path@.to_bytes(), index + 1, dep_path@.to_bytes().len()) != 0,
    None => dep_path@.to_bytes().len() == 0
        || dep_path@.to_bytes()[dep_path@.to_bytes().len() - 1] != CLOSE
        || forall<index> 0 <= index && index < dep_path@.to_bytes().len()
            && !is_paren(dep_path@.to_bytes()[index]) ==>
            excess(dep_path@.to_bytes(), index + 1, dep_path@.to_bytes().len()) != 0,
}))]
#[must_use]
pub fn dep_path_suffix_start(dep_path: &str) -> Option<usize> {
    let bytes = dep_path.as_bytes();
    let last = bytes.len().checked_sub(1)?;
    if bytes[last] != CLOSE {
        return None;
    }
    #[cfg(creusot)]
    proof_assert! { str_size_bound(dep_path); bytes@.len() <= isize::MAX@ };
    let mut open: isize = 1;
    let mut index = last;
    #[cfg_attr(creusot, invariant(index@ <= last@))]
    #[cfg_attr(creusot, invariant(open@ == excess(bytes@, index@, bytes@.len())))]
    #[cfg_attr(creusot, invariant(-(bytes@.len() - index@) <= open@ && open@ <= bytes@.len() - index@))]
    #[cfg_attr(creusot, invariant(forall<later> index@ <= later && later < bytes@.len()
        && !is_paren(bytes@[later]) ==> excess(bytes@, later + 1, bytes@.len()) != 0))]
    while index > 0 {
        index -= 1;
        match bytes[index] {
            OPEN => open -= 1,
            CLOSE => open += 1,
            _ if open == 0 => {
                #[cfg(creusot)]
                proof_assert! { ascii_byte(dep_path@, index@ + 1); is_boundary(dep_path@, index@ + 1) };
                return Some(index + 1);
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests;
