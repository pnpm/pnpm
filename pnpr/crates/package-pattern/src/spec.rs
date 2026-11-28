//! What a pattern matches, stated over a name's characters, and the lemmas
//! that find where a name's first `/` is.

use crate::{
    Namespace,
    Scope,
    Shape,
};
use creusot_std::prelude::{
    Int,
    Invariant,
    Seq,
    View,
    ensures,
    logic,
    requires,
    variant,
};

impl View for Scope {
    type ViewTy = Seq<char>;
    #[logic(open(crate))]
    fn view(self) -> Seq<char> {
        pearlite! { self.0@ }
    }
}

impl Invariant for Scope {
    #[logic(open)]
    fn invariant(self) -> bool {
        pearlite! { is_scope(self@) }
    }
}

impl View for Namespace {
    type ViewTy = Seq<char>;
    #[logic(open(crate))]
    fn view(self) -> Seq<char> {
        pearlite! { self.0@ }
    }
}

impl Invariant for Namespace {
    #[logic(open)]
    fn invariant(self) -> bool {
        pearlite! { is_namespace(self@) }
    }
}

/// Whether `text` can be a pattern scope: non-empty, with no `/`.
#[logic(open)]
pub fn is_scope(text: Seq<char>) -> bool {
    pearlite! { text.len() > 0 && !text.contains('/') }
}

/// Whether `text` can be a pattern namespace: non-empty, with neither `/`
/// nor `@`.
#[logic(open)]
pub fn is_namespace(text: Seq<char>) -> bool {
    pearlite! { text.len() > 0 && !text.contains('/') && !text.contains('@') }
}

/// The index of the first `/` in `text`, or its length when it has none.
#[logic(open)]
#[variant(text.len())]
pub fn first_slash(text: Seq<char>) -> Int {
    pearlite! {
        if text.len() == 0 || text[0] == '/' { 0 } else { first_slash(text.tail()) + 1 }
    }
}

/// Whether `name` is `@{scope}/{rest}` with a non-empty scope and rest, and
/// `scope` the part before its first `/`.
#[logic(open)]
pub fn scoped_in(name: Seq<char>, scope: Seq<char>) -> bool {
    pearlite! {
        name.len() > 0 && name[0] == '@'
            && 0 < first_slash(name.tail()) && first_slash(name.tail()) + 1 < name.tail().len()
            && scope == name.tail().subsequence(0, first_slash(name.tail()))
    }
}

/// Whether `name` is a well-formed scoped name.
#[logic(open)]
pub fn is_scoped(name: Seq<char>) -> bool {
    pearlite! { scoped_in(name, name.tail().subsequence(0, first_slash(name.tail()))) }
}

/// Whether `name` has a `/`, and `namespace` is the part before the first.
#[logic(open)]
pub fn in_namespace(name: Seq<char>, namespace: Seq<char>) -> bool {
    pearlite! {
        first_slash(name) < name.len() && namespace == name.subsequence(0, first_slash(name))
    }
}

/// Whether `shape` matches `name`.
#[logic(open)]
pub fn matches_name(shape: Shape<'_>, name: Seq<char>) -> bool {
    pearlite! {
        match shape {
            Shape::All => true,
            Shape::AnyScoped => is_scoped(name),
            Shape::Scope(scope) => scoped_in(name, scope@),
            Shape::Namespace(namespace) => in_namespace(name, namespace@),
            Shape::Exact(exact) => name == exact@,
        }
    }
}

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

/// `{text}/x`: a name whose first `/` ends `text` when `text` has none.
#[logic(open)]
pub fn with_child(text: Seq<char>) -> Seq<char> {
    pearlite! { text.concat(Seq::singleton('/')).concat(Seq::singleton('x')) }
}

/// `@{scope}/x`.
#[logic(open)]
pub fn scoped_child(scope: Seq<char>) -> Seq<char> {
    pearlite! { Seq::singleton('@').concat(with_child(scope)) }
}

/// `{text}x`, which is never `text`.
#[logic(open)]
pub fn longer(text: Seq<char>) -> Seq<char> {
    pearlite! { text.concat(Seq::singleton('x')) }
}

/// `name`, or `longer(name)` when `name` is `avoid`: a name that is not
/// `avoid`.
#[logic(open)]
pub fn other_than(name: Seq<char>, avoid: Seq<char>) -> Seq<char> {
    pearlite! { if name == avoid { longer(name) } else { name } }
}

/// Whether a shape's scope or namespace holds what its type promises.
#[logic(open)]
pub fn well_formed(shape: Shape<'_>) -> bool {
    pearlite! {
        match shape {
            Shape::Scope(scope) => is_scope(scope@),
            Shape::Namespace(namespace) => is_namespace(namespace@),
            _ => true,
        }
    }
}

/// The rule [`crate::covers`] decides by.
#[logic(open)]
pub fn covered(pattern: Shape<'_>, other: Shape<'_>) -> bool {
    pearlite! {
        match (pattern, other) {
            (Shape::All, _) => true,
            (Shape::AnyScoped, Shape::AnyScoped) | (Shape::AnyScoped, Shape::Scope(_)) => true,
            (Shape::AnyScoped, Shape::Exact(name)) => is_scoped(name@),
            (Shape::Scope(scope), Shape::Scope(other_scope)) => scope@ == other_scope@,
            (Shape::Scope(scope), Shape::Exact(name)) => scoped_in(name@, scope@),
            (Shape::Namespace(namespace), Shape::Namespace(other_namespace)) =>
                namespace@ == other_namespace@,
            (Shape::Namespace(namespace), Shape::Exact(name)) => in_namespace(name@, namespace@),
            (Shape::Exact(name), Shape::Exact(other_name)) => name@ == other_name@,
            _ => false,
        }
    }
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

/// A name `other` matches: `x`, `@a/x`, `@{scope}/x`, `{namespace}/x`, or
/// the exact name.
#[logic(open)]
pub fn sample(other: Shape<'_>) -> Seq<char> {
    pearlite! {
        match other {
            Shape::All => Seq::singleton('x'),
            Shape::AnyScoped => scoped_child(Seq::singleton('a')),
            Shape::Scope(scope) => scoped_child(scope@),
            Shape::Namespace(namespace) => with_child(namespace@),
            Shape::Exact(name) => name@,
        }
    }
}

/// A name `other` matches and `pattern` does not, when `pattern` does not
/// cover `other`.
#[logic(open)]
pub fn counterexample(pattern: Shape<'_>, other: Shape<'_>) -> Seq<char> {
    pearlite! {
        match pattern {
            Shape::Exact(name) => other_than(sample(other), name@),
            Shape::Scope(scope) => match other {
                Shape::AnyScoped => scoped_child(longer(scope@)),
                _ => sample(other),
            },
            _ => sample(other),
        }
    }
}

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
