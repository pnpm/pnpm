//! The lemmas that find where a name's first `/` is, and that show `covers`
//! holds exactly when one pattern matches every name another does.

use super::logic::{
    counterexample,
    covered,
    first_slash,
    in_namespace,
    is_namespace,
    is_scope,
    is_scoped,
    longer,
    matches_name,
    scoped_child,
    scoped_in,
    well_formed,
    with_child,
};
use crate::Shape;
use creusot_std::prelude::{
    Seq,
    ensures,
    logic,
    requires,
    variant,
};

/// A non-empty `front` in front of `rest` starts as `front` does, and the
/// tail is the rest of `front` and then `rest`.
#[logic]
#[requires(front.len() > 0)]
#[ensures(front.concat(rest)[0] == front[0])]
#[ensures(front.concat(rest).tail().ext_eq(front.tail().concat(rest)))]
#[ensures(front.concat(rest).tail() == front.tail().concat(rest))]
pub fn tail_of_concat(front: Seq<char>, rest: Seq<char>) {}

/// `front` is the first `front.len()` characters of `front` and then `rest`.
#[logic]
#[ensures(front.concat(rest).subsequence(0, front.len()).ext_eq(front))]
#[ensures(front.concat(rest).subsequence(0, front.len()) == front)]
pub fn prefix_of_concat(front: Seq<char>, rest: Seq<char>) {}

/// An empty text is `Seq::empty()`.
#[logic]
#[requires(text.len() == 0)]
#[ensures(text.ext_eq(Seq::empty()))]
#[ensures(text == Seq::empty())]
pub fn empty_text(text: Seq<char>) {}

/// A text with no `/` in front of a `/` has its first `/` right after it.
#[logic]
#[variant(before.len())]
#[requires(!before.contains('/'))]
#[ensures(first_slash(before.concat(Seq::singleton('/')).concat(after)) == before.len())]
pub fn first_slash_after(before: Seq<char>, after: Seq<char>) {
    pearlite! {
        before.concat_assoc(Seq::singleton('/'), after);
        if before.len() > 0 {
            tail_of_concat(before, Seq::singleton('/').concat(after));
            first_slash_after(before.tail(), after);
            before.tail().concat_assoc(Seq::singleton('/'), after)
        } else {
            empty_text(before);
            Seq::singleton('/').concat(after).concat_empty()
        }
    }
}

/// `{before}/{after}` with no `/` in `before` has its first `/` after
/// `before`, which is its prefix of that length.
#[logic]
#[requires(!before.contains('/'))]
#[ensures(first_slash(before.concat(Seq::singleton('/')).concat(after)) == before.len())]
#[ensures(before.concat(Seq::singleton('/')).concat(after).subsequence(0, before.len()) == before)]
#[ensures(before.concat(Seq::singleton('/')).concat(after).len() == before.len() + 1 + after.len())]
pub fn slash_split(before: Seq<char>, after: Seq<char>) {
    pearlite! {
        first_slash_after(before, after);
        before.concat_assoc(Seq::singleton('/'), after);
        prefix_of_concat(before, Seq::singleton('/').concat(after))
    }
}

/// A character in front of `rest` is the first character, and `rest` the
/// tail.
#[logic]
#[ensures(Seq::singleton(first).concat(rest)[0] == first)]
#[ensures(Seq::singleton(first).concat(rest).tail().ext_eq(rest))]
#[ensures(Seq::singleton(first).concat(rest).tail() == rest)]
pub fn cons_tail(first: char, rest: Seq<char>) {}

/// A text with no `/` has none to find.
#[logic]
#[variant(text.len())]
#[requires(!text.contains('/'))]
#[ensures(first_slash(text) == text.len())]
pub fn no_slash(text: Seq<char>) {
    pearlite! { if text.len() > 0 { no_slash(text.tail()) } }
}

/// `x` is neither scoped nor in a namespace.
#[logic]
#[ensures(!is_scoped(Seq::singleton('x')))]
#[ensures(forall<namespace: Seq<char>> !in_namespace(Seq::singleton('x'), namespace))]
pub fn plain_name() {
    pearlite! { no_slash(Seq::singleton('x')) }
}

/// `{namespace}/x` and `{namespace}/xx` are in `namespace` and not scoped.
#[logic]
#[requires(is_namespace(namespace))]
#[ensures(in_namespace(with_child(namespace), namespace))]
#[ensures(in_namespace(longer(with_child(namespace)), namespace))]
#[ensures(!is_scoped(with_child(namespace)) && !is_scoped(longer(with_child(namespace))))]
pub fn namespace_child(namespace: Seq<char>) {
    pearlite! {
        slash_split(namespace, Seq::singleton('x'));
        slash_split(namespace, Seq::singleton('x').concat(Seq::singleton('x')));
        namespace.concat(Seq::singleton('/')).concat_assoc(Seq::singleton('x'), Seq::singleton('x'));
        tail_of_concat(namespace, Seq::singleton('/').concat(Seq::singleton('x')));
        namespace.concat_assoc(Seq::singleton('/'), Seq::singleton('x'))
    }
}

/// `@{scope}/x` and `@{scope}/xx` are in `scope`, and in no namespace
/// without `@`.
#[logic]
#[requires(is_scope(scope))]
#[ensures(scoped_in(scoped_child(scope), scope))]
#[ensures(scoped_in(longer(scoped_child(scope)), scope))]
#[ensures(forall<namespace: Seq<char>> !namespace.contains('@') ==>
    !in_namespace(scoped_child(scope), namespace)
        && !in_namespace(longer(scoped_child(scope)), namespace))]
pub fn scope_child(scope: Seq<char>) {
    pearlite! {
        slash_split(scope, Seq::singleton('x'));
        slash_split(scope, Seq::singleton('x').concat(Seq::singleton('x')));
        scope.concat(Seq::singleton('/')).concat_assoc(Seq::singleton('x'), Seq::singleton('x'));
        cons_tail('@', with_child(scope));
        Seq::singleton('@').concat_assoc(with_child(scope), Seq::singleton('x'));
        cons_tail('@', longer(with_child(scope)))
    }
}

/// A name in a scope is scoped, and in that scope only.
#[logic]
#[ensures(forall<name: Seq<char>, scope: Seq<char>> scoped_in(name, scope) ==> is_scoped(name))]
#[ensures(forall<name: Seq<char>, scope: Seq<char>, other: Seq<char>>
    scoped_in(name, scope) && scoped_in(name, other) ==> scope == other)]
#[ensures(forall<name: Seq<char>, namespace: Seq<char>, other: Seq<char>>
    in_namespace(name, namespace) && in_namespace(name, other) ==> namespace == other)]
pub fn one_scope() {}

/// Where `pattern` does not cover `other`, [`counterexample`] is a name
/// `other` matches and `pattern` does not.
#[logic]
#[requires(well_formed(pattern) && well_formed(other) && !covered(pattern, other))]
#[ensures(matches_name(other, counterexample(pattern, other)))]
#[ensures(!matches_name(pattern, counterexample(pattern, other)))]
pub fn counterexample_works(pattern: Shape<'_>, other: Shape<'_>) {
    pearlite! {
        plain_name();
        one_scope();
        scope_child(Seq::singleton('a'));
        match other {
            Shape::Scope(scope) => scope_child(scope@),
            Shape::Namespace(namespace) => namespace_child(namespace@),
            _ => (),
        };
        match pattern {
            Shape::Scope(scope) => {
                scope@.concat_contains(Seq::singleton('x'), '/');
                scope_child(longer(scope@))
            }
            _ => (),
        }
    }
}

/// [`covered`] is inclusion: `pattern` covers `other` exactly when it
/// matches every name `other` matches.
#[logic]
#[requires(well_formed(pattern) && well_formed(other))]
#[ensures(covered(pattern, other)
    == forall<name: Seq<char>> matches_name(other, name) ==> matches_name(pattern, name))]
pub fn covered_is_inclusion(pattern: Shape<'_>, other: Shape<'_>) {
    pearlite! {
        one_scope();
        if !covered(pattern, other) { counterexample_works(pattern, other) }
    }
}
