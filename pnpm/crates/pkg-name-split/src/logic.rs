//! The type invariants of the split's parts.

use crate::{
    KeyName,
    Scope,
};
use creusot_std::prelude::{
    Invariant,
    Seq,
    View,
    logic,
};

/// What a [`KeyName`] holds: a non-empty name with no `@` after its first
/// character.
#[logic(open)]
pub fn is_key_name(text: Seq<char>) -> bool {
    pearlite! { text.len() > 0 && !text.subsequence(1, text.len()).contains('@') }
}

/// What a [`Scope`] holds: no `/`.
#[logic(open)]
pub fn is_scope(text: Seq<char>) -> bool {
    pearlite! { !text.contains('/') }
}

impl View for KeyName<'_> {
    type ViewTy = Seq<char>;
    #[logic(open(crate))]
    fn view(self) -> Seq<char> {
        pearlite! { self.0@ }
    }
}

impl Invariant for KeyName<'_> {
    #[logic(open)]
    fn invariant(self) -> bool {
        pearlite! { is_key_name(self@) }
    }
}

impl View for Scope<'_> {
    type ViewTy = Seq<char>;
    #[logic(open(crate))]
    fn view(self) -> Seq<char> {
        pearlite! { self.0@ }
    }
}

impl Invariant for Scope<'_> {
    #[logic(open)]
    fn invariant(self) -> bool {
        pearlite! { is_scope(self@) }
    }
}
