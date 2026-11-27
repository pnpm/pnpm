//! Where a lockfile package key splits: the scope and bare name of
//! `@{scope}/{bare}`, and the name and suffix of `{name}@{suffix}`.
//!
//! Creusot proves both splits, see `pnpm/VERIFICATION.md`. Each part is a
//! substring of the input, the parts and their separator rejoin to the
//! input, and the split is at the first separator the syntax allows there.
//! Neither function panics, the slicing included.

#[cfg(creusot)]
pub mod spec;
#[cfg(creusot)]
pub mod theorems;

#[cfg(creusot)]
use creusot_std::prelude::{
    Seq,
    ensures,
    proof_assert,
};
#[cfg(creusot)]
use pnpm_creusot_models as _;
use split_first_char::SplitFirstChar;

/// Where the scope of a package name ends.
#[derive(Debug, Clone, Copy)]
pub enum ScopeSplit<'name> {
    /// `@{scope}/{bare}`.
    Scoped { scope: &'name str, bare: &'name str },
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
    Ok(ScopeSplit::Scoped { scope, bare }) => !scope@.contains('/')
        && name@ == Seq::singleton('@').concat(scope@).concat(Seq::singleton('/')).concat(bare@),
}))]
pub fn split_scope(name: &str) -> Result<ScopeSplit<'_>, SplitScopeError> {
    let Some((first, rest)) = name.split_first_char() else {
        return Err(SplitScopeError::Empty);
    };
    if first != '@' {
        return Ok(ScopeSplit::Unscoped);
    }
    match rest.split_once('/') {
        Some((scope, bare)) => Ok(ScopeSplit::Scoped { scope, bare }),
        None => Err(SplitScopeError::MissingBare),
    }
}

/// Why a lockfile key has no name and suffix split.
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
    Ok((name, suffix)) => name@.len() > 0
        && !name@.subsequence(1, name@.len()).contains('@')
        && key@ == name@.concat(Seq::singleton('@')).concat(suffix@),
}))]
pub fn split_name_suffix(key: &str) -> Result<(&str, &str), SplitNameSuffixError> {
    let Some((first, rest)) = key.split_first_char() else {
        return Err(SplitNameSuffixError::EmptyInput);
    };
    if first != '@' {
        return key.split_once('@').ok_or(SplitNameSuffixError::MissingSuffix);
    }
    let Some((name_without_at, suffix)) = rest.split_once('@') else {
        return Err(SplitNameSuffixError::MissingSuffix);
    };
    #[cfg(creusot)]
    proof_assert! {
        spec::scoped_name_bytes(key, rest@, name_without_at@, suffix@);
        key@.subsequence(0, name_without_at@.len() + 1).to_bytes().len()
            == name_without_at@.to_bytes().len() + 1
            && name_without_at@.to_bytes().len() + 1 <= usize::MAX@
    };
    let (name, _at_and_suffix) = key.split_at(name_without_at.len() + 1);
    #[cfg(creusot)]
    proof_assert! {
        spec::scoped_name_bytes(key, rest@, name_without_at@, suffix@);
        spec::equal_byte_prefixes(
            name@,
            _at_and_suffix@,
            Seq::singleton('@').concat(name_without_at@),
            Seq::singleton('@').concat(suffix@),
        );
        name@ == Seq::singleton('@').concat(name_without_at@)
    };
    Ok((name, suffix))
}

#[cfg(test)]
mod tests;
