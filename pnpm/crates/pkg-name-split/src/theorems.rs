//! Properties stated for every input, each as a verified caller.

use crate::{
    ScopeSplit,
    SplitNameSuffixError,
    SplitScopeError,
    spec::{
        scope_split_is_unique,
        split_is_unique,
    },
    split_name_suffix,
    split_scope,
};
use creusot_std::prelude::{
    Seq,
    ensures,
    proof_assert,
    requires,
};

/// A key written as `{name}@{suffix}` reads back as that name and suffix
/// whenever the name is non-empty and holds no `@` after its first
/// character, which every package name satisfies.
#[requires(name@.len() > 0 && !name@.subsequence(1, name@.len()).contains('@'))]
#[requires(key@ == name@.concat(Seq::singleton('@')).concat(suffix@))]
#[ensures(match result {
    Ok((read_name, read_suffix)) => read_name@ == name@ && read_suffix@ == suffix@,
    Err(_) => false,
})]
pub fn written_key_reads_back<'key>(
    key: &'key str,
    name: &str,
    suffix: &str,
) -> Result<(&'key str, &'key str), SplitNameSuffixError> {
    proof_assert!(key@.subsequence(1, key@.len())[name@.len() - 1] == '@');
    let result = split_name_suffix(key);
    proof_assert! {
        match result {
            Ok((read_name, read_suffix)) => {
                split_is_unique(read_name@, read_suffix@, name@, suffix@, '@');
                read_name@ == name@ && read_suffix@ == suffix@
            }
            Err(_) => true,
        }
    };
    result
}

/// A name written as `@{scope}/{bare}` reads back as that scope and bare
/// name whenever the scope holds no `/`.
#[requires(!scope@.contains('/'))]
#[requires(name@ == Seq::singleton('@').concat(scope@).concat(Seq::singleton('/')).concat(bare@))]
#[ensures(match result {
    Ok(ScopeSplit::Scoped { scope: read_scope, bare: read_bare }) =>
        read_scope@ == scope@ && read_bare@ == bare@,
    _ => false,
})]
pub fn written_name_reads_back<'name>(
    name: &'name str,
    scope: &str,
    bare: &str,
) -> Result<ScopeSplit<'name>, SplitScopeError> {
    proof_assert!(name@.subsequence(1, name@.len())[scope@.len()] == '/');
    let result = split_scope(name);
    proof_assert! {
        match result {
            Ok(ScopeSplit::Scoped { scope: read_scope, bare: read_bare }) => {
                scope_split_is_unique(read_scope@, read_bare@, scope@, bare@);
                read_scope@ == scope@ && read_bare@ == bare@
            }
            _ => true,
        }
    };
    result
}
