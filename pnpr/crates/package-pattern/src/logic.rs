//! What a pattern matches, stated over a name's characters, the rule
//! `covers` decides by, and the names its proof builds as witnesses.

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
    logic,
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
