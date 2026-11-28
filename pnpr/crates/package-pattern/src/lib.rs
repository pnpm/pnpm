//! What a registry's package patterns match, and when one covers another.
//!
//! Creusot proves, see `pnpm/VERIFICATION.md`, that no function here panics
//! and that [`covers`] is exactly inclusion: `covers(pattern, other)` holds
//! if and only if `pattern` matches every package name `other` matches.
//!
//! That proof needs two facts, which [`Scope`] and [`Namespace`] carry as
//! their type invariants: a scope holds no `/`, and a namespace holds neither
//! `/` nor `@`. A namespace `@acme` would match every name in the scope
//! `acme`, and [`covers`] would not report that it covers `@acme/*`.

#[cfg(creusot)]
pub mod spec;

#[cfg(creusot)]
use creusot_std::prelude::{
    Seq,
    ensures,
    proof_assert,
};
#[cfg(creusot)]
use pnpm_creusot_models as _;
#[cfg(creusot)]
use spec::{
    cons_tail,
    covered,
    covered_is_inclusion,
    first_slash,
    in_namespace,
    is_namespace,
    is_scope,
    is_scoped,
    matches_name,
    no_slash,
    scoped_in,
    slash_split,
};
use split_first_char::SplitFirstChar;

/// The scope of an `@<scope>/*` pattern, without its `@`: non-empty, with no
/// `/`.
#[cfg_attr(not(creusot), derive(Debug, Clone, PartialEq, Eq))]
pub struct Scope(String);

impl Scope {
    /// `scope` as a pattern scope, if it is non-empty and holds no `/`.
    #[cfg_attr(creusot, ensures(match result {
        Some(made) => made@ == scope@,
        None => !is_scope(scope@),
    }))]
    #[must_use]
    pub fn new(scope: String) -> Option<Self> {
        if scope.is_empty() || scope.contains('/') { None } else { Some(Scope(scope)) }
    }

    /// The scope as written, without its `@`.
    #[cfg_attr(creusot, ensures(result@ == self@))]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The leading path component of a `<namespace>/*` pattern: non-empty, with
/// neither `/` nor `@`.
#[cfg_attr(not(creusot), derive(Debug, Clone, PartialEq, Eq))]
pub struct Namespace(String);

impl Namespace {
    /// `namespace` as a pattern namespace, if it is non-empty and holds
    /// neither `/` nor `@`.
    #[cfg_attr(creusot, ensures(match result {
        Some(made) => made@ == namespace@,
        None => !is_namespace(namespace@),
    }))]
    #[must_use]
    pub fn new(namespace: String) -> Option<Self> {
        if namespace.is_empty() || namespace.contains(['/', '@']) {
            None
        } else {
            Some(Namespace(namespace))
        }
    }

    /// The namespace as written.
    #[cfg_attr(creusot, ensures(result@ == self@))]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The shape of a package pattern.
#[derive(Clone, Copy)]
pub enum Shape<'pattern> {
    /// `**`: every package name.
    All,
    /// `@*/*`: every scoped package, any scope.
    AnyScoped,
    /// `@<scope>/*`: every package in one scope.
    Scope(&'pattern Scope),
    /// `<namespace>/*`: every name whose leading path component is the
    /// namespace.
    Namespace(&'pattern Namespace),
    /// One package name.
    Exact(&'pattern str),
}

/// Whether `shape` matches `package`.
#[cfg_attr(creusot, ensures(result == matches_name(shape, package@)))]
#[must_use]
pub fn matches(shape: Shape<'_>, package: &str) -> bool {
    match shape {
        Shape::All => true,
        Shape::AnyScoped => split_scoped_name(package).is_some(),
        Shape::Scope(scope) => match split_scoped_name(package) {
            Some((found, _)) => found == scope.as_str(),
            None => false,
        },
        Shape::Namespace(namespace) => match split_namespace(package) {
            Some(found) => found == namespace.as_str(),
            None => false,
        },
        Shape::Exact(name) => name == package,
    }
}

/// Whether `pattern` matches every package name `other` matches.
#[cfg_attr(creusot, ensures(result == forall<name: Seq<char>>
    matches_name(other, name) ==> matches_name(pattern, name)))]
#[must_use]
pub fn covers(pattern: Shape<'_>, other: Shape<'_>) -> bool {
    #[cfg(creusot)]
    proof_assert! {
        covered_is_inclusion(pattern, other);
        covered(pattern, other) == forall<name: Seq<char>>
            matches_name(other, name) ==> matches_name(pattern, name)
    };
    match (pattern, other) {
        (Shape::All, _) => true,
        (Shape::AnyScoped, Shape::AnyScoped | Shape::Scope(_)) => true,
        (Shape::AnyScoped, Shape::Exact(name)) => split_scoped_name(name).is_some(),
        (Shape::Scope(scope), Shape::Scope(other_scope)) => scope.as_str() == other_scope.as_str(),
        (Shape::Scope(scope), Shape::Exact(name)) => match split_scoped_name(name) {
            Some((found, _)) => found == scope.as_str(),
            None => false,
        },
        (Shape::Namespace(namespace), Shape::Namespace(other_namespace)) => {
            namespace.as_str() == other_namespace.as_str()
        }
        (Shape::Namespace(namespace), Shape::Exact(name)) => match split_namespace(name) {
            Some(found) => found == namespace.as_str(),
            None => false,
        },
        (Shape::Exact(name), Shape::Exact(other_name)) => name == other_name,
        _ => false,
    }
}

/// The `(scope, name)` of a well-formed scoped package (`@acme/foo` gives
/// `("acme", "foo")`), or `None` when it is unscoped or missing either part.
#[cfg_attr(creusot, ensures(match result {
    Some((scope, _)) => scoped_in(package@, scope@),
    None => !is_scoped(package@),
}))]
#[must_use]
pub fn split_scoped_name(package: &str) -> Option<(&str, &str)> {
    let (first, rest) = package.split_first_char()?;
    if first != '@' {
        return None;
    }
    #[cfg(creusot)]
    proof_assert! { cons_tail(first, rest@); package@[0] == '@' && package@.tail() == rest@ };
    let Some((scope, name)) = rest.split_once('/') else {
        #[cfg(creusot)]
        proof_assert! { no_slash(rest@); first_slash(rest@) == rest@.len() };
        return None;
    };
    #[cfg(creusot)]
    proof_assert! {
        slash_split(scope@, name@);
        first_slash(rest@) == scope@.len() && rest@.subsequence(0, scope@.len()) == scope@
            && rest@.len() == scope@.len() + 1 + name@.len()
    };
    if scope.is_empty() || name.is_empty() { None } else { Some((scope, name)) }
}

/// The leading path component of `package`, or `None` when it has one
/// component only.
#[cfg_attr(creusot, ensures(match result {
    Some(namespace) => in_namespace(package@, namespace@),
    None => first_slash(package@) == package@.len(),
}))]
#[must_use]
pub fn split_namespace(package: &str) -> Option<&str> {
    match package.split_once('/') {
        Some((namespace, _rest)) => {
            #[cfg(creusot)]
            proof_assert! {
                slash_split(namespace@, _rest@);
                in_namespace(package@, namespace@)
            };
            Some(namespace)
        }
        None => {
            #[cfg(creusot)]
            proof_assert! { no_slash(package@); first_slash(package@) == package@.len() };
            None
        }
    }
}

#[cfg(test)]
mod tests;
