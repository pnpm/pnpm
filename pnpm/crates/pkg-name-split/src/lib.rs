//! Where a package name or a `{name}@{suffix}` key splits: the scope and
//! bare name of `@{scope}/{bare}`, and the name and suffix of
//! `{name}@{suffix}`, as lockfile keys, specifiers and descriptors write it.
//!
//! Creusot proves each split, see `pnpm/VERIFICATION.md`. Each part is a
//! substring of the input, the parts and their separator rejoin to the
//! input, and the split is at the first separator the syntax allows there.
//! Neither function panics, the slicing included.
//!
//! [`KeyName`] and [`Scope`] carry what makes each split unique as their
//! type invariant, so a verified caller that holds one has the fact without
//! restating it.
//!
//! [`groups`] splits the parenthesized suffix of a dep path. Creusot proves
//! that its functions do not panic, that each group it reports opens with
//! the `(` that its `)` closes, that the groups and the text in front of
//! them are the input's bytes in order, and where the suffix scan stops.

pub mod groups;
#[cfg(creusot)]
pub mod lemmas;
#[cfg(creusot)]
pub mod logic;

#[cfg(creusot)]
use creusot_std::prelude::{
    Seq,
    ensures,
    proof_assert,
    requires,
};
#[cfg(creusot)]
use pnpm_creusot_models as _;
use split_first_char::SplitFirstChar;

/// The scope of `@{scope}/{bare}`, which holds no `/`.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'name>(&'name str);

impl<'name> Scope<'name> {
    /// The only constructor, private for the reason `pnpm/VERIFICATION.md`
    /// gives.
    #[cfg_attr(creusot, requires(logic::is_scope(text@)))]
    #[cfg_attr(creusot, ensures(result@ == text@))]
    fn new(text: &'name str) -> Self {
        Scope(text)
    }

    /// The scope as written in the name, without its `@`.
    #[cfg_attr(creusot, ensures(result@ == self@))]
    #[must_use]
    pub fn as_str(self) -> &'name str {
        self.0
    }
}

/// Where the scope of a package name ends.
#[derive(Debug, Clone, Copy)]
pub enum ScopeSplit<'name> {
    /// `@{scope}/{bare}`.
    Scoped { scope: Scope<'name>, bare: &'name str },
    /// A name with no leading `@`, which is a bare name throughout.
    Unscoped,
}

/// Why a package name has no scope split.
#[derive(Debug, Clone, Copy)]
pub enum SplitScopeError {
    /// The name is empty.
    Empty,
    /// The name starts with `@` and holds no `/`.
    MissingBare,
}

/// Split `@{scope}/{bare}` at the first `/`, or report a name without a
/// leading `@` as unscoped.
#[cfg_attr(creusot, ensures(match result {
    Err(SplitScopeError::Empty) => name@.len() == 0,
    Err(SplitScopeError::MissingBare) => name@.len() > 0 && name@[0] == '@'
        && !name@.subsequence(1, name@.len()).contains('/'),
    Ok(ScopeSplit::Unscoped) => name@.len() > 0 && name@[0] != '@',
    Ok(ScopeSplit::Scoped { scope, bare }) =>
        name@ == Seq::singleton('@').concat(scope@).concat(Seq::singleton('/')).concat(bare@),
}))]
pub fn split_scope(name: &str) -> Result<ScopeSplit<'_>, SplitScopeError> {
    let Some((first, rest)) = name.split_first_char() else {
        return Err(SplitScopeError::Empty);
    };
    if first != '@' {
        return Ok(ScopeSplit::Unscoped);
    }
    match rest.split_once('/') {
        Some((scope, bare)) => Ok(ScopeSplit::Scoped { scope: Scope::new(scope), bare }),
        None => Err(SplitScopeError::MissingBare),
    }
}

/// The name of a `{name}@{suffix}` key, which is non-empty and holds no `@`
/// after its first character. A scoped name keeps its leading `@`.
#[derive(Debug, Clone, Copy)]
pub struct KeyName<'key>(&'key str);

impl<'key> KeyName<'key> {
    /// The only constructor, private for the reason `pnpm/VERIFICATION.md`
    /// gives.
    #[cfg_attr(creusot, requires(logic::is_key_name(text@)))]
    #[cfg_attr(creusot, ensures(result@ == text@))]
    fn new(text: &'key str) -> Self {
        KeyName(text)
    }

    /// The name as written in the key.
    #[cfg_attr(creusot, ensures(result@ == self@))]
    #[must_use]
    pub fn as_str(self) -> &'key str {
        self.0
    }
}

/// Why a key has no name and suffix split.
#[derive(Debug, Clone, Copy)]
pub enum SplitNameSuffixError {
    /// The key is empty.
    EmptyInput,
    /// The key holds no `@` after its first character.
    MissingSuffix,
}

/// Split `{name}@{suffix}` at the first `@` after the first character, so
/// the leading `@` of a scoped name stays in the name.
#[cfg_attr(creusot, ensures(match result {
    Err(SplitNameSuffixError::EmptyInput) => key@.len() == 0,
    Err(SplitNameSuffixError::MissingSuffix) => key@.len() > 0
        && !key@.subsequence(1, key@.len()).contains('@'),
    Ok((name, suffix)) => key@ == name@.concat(Seq::singleton('@')).concat(suffix@),
}))]
pub fn split_name_suffix(key: &str) -> Result<(KeyName<'_>, &str), SplitNameSuffixError> {
    let Some((first, rest)) = key.split_first_char() else {
        return Err(SplitNameSuffixError::EmptyInput);
    };
    if first != '@' {
        let Some((name, suffix)) = key.split_once('@') else {
            return Err(SplitNameSuffixError::MissingSuffix);
        };
        return Ok((KeyName::new(name), suffix));
    }
    let Some((name_without_at, suffix)) = rest.split_once('@') else {
        return Err(SplitNameSuffixError::MissingSuffix);
    };
    #[cfg(creusot)]
    proof_assert! {
        lemmas::scoped_name_bytes(key, rest@, name_without_at@, suffix@);
        key@.subsequence(0, name_without_at@.len() + 1).to_bytes().len()
            == name_without_at@.to_bytes().len() + 1
            && name_without_at@.to_bytes().len() + 1 <= usize::MAX@
    };
    let (name, _at_and_suffix) = key.split_at(name_without_at.len() + 1);
    #[cfg(creusot)]
    proof_assert! {
        lemmas::scoped_name_bytes(key, rest@, name_without_at@, suffix@);
        lemmas::equal_byte_prefixes(
            name@,
            _at_and_suffix@,
            Seq::singleton('@').concat(name_without_at@),
            Seq::singleton('@').concat(suffix@),
        );
        name@.concat_assoc(Seq::singleton('@'), suffix@);
        name@ == Seq::singleton('@').concat(name_without_at@)
            && key@ == name@.concat(Seq::singleton('@')).concat(suffix@)
    };
    Ok((KeyName::new(name), suffix))
}

#[cfg(test)]
mod tests;
#[cfg(creusot)]
#[expect(dead_code, reason = "a theorem is proved, never called")]
mod theorems;
