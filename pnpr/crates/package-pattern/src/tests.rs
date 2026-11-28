use super::{
    Namespace,
    Scope,
    Shape,
    covers,
    matches,
    split_namespace,
    split_scoped_name,
};
use pretty_assertions::assert_eq;

fn scope(text: &str) -> Scope {
    Scope::new(text.to_string()).expect("a valid scope")
}

fn namespace(text: &str) -> Namespace {
    Namespace::new(text.to_string()).expect("a valid namespace")
}

#[test]
fn a_scope_is_neither_empty_nor_holds_a_slash() {
    assert!(Scope::new(String::new()).is_none());
    assert!(Scope::new("a/b".to_string()).is_none());
    assert_eq!(scope("@a.b").as_str(), "@a.b");
}

#[test]
fn a_namespace_is_neither_empty_nor_holds_a_slash_or_an_at() {
    assert!(Namespace::new(String::new()).is_none());
    assert!(Namespace::new("a/b".to_string()).is_none());
    assert!(Namespace::new("@acme".to_string()).is_none());
    assert_eq!(namespace("a.b").as_str(), "a.b");
}

#[test]
fn splits_scoped_names_and_namespaces() {
    assert_eq!(split_scoped_name("@acme/foo"), Some(("acme", "foo")));
    assert_eq!(split_scoped_name("@acme/foo/bar"), Some(("acme", "foo/bar")));
    for name in ["foo", "@acme", "@/foo", "@acme/", ""] {
        assert_eq!(split_scoped_name(name), None, "{name:?}");
    }
    assert_eq!(split_namespace("acme/app/web"), Some("acme"));
    assert_eq!(split_namespace("app"), None);
}

#[test]
fn matches_each_shape() {
    let acme = scope("acme");
    let images = namespace("acme");
    assert!(matches(Shape::All, "anything"));
    assert!(matches(Shape::AnyScoped, "@a/b"));
    assert!(!matches(Shape::AnyScoped, "@a"));
    assert!(matches(Shape::Scope(&acme), "@acme/foo"));
    assert!(!matches(Shape::Scope(&acme), "@acmex/foo"));
    assert!(matches(Shape::Namespace(&images), "acme/app"));
    assert!(!matches(Shape::Namespace(&images), "acme"));
    assert!(matches(Shape::Exact("foo"), "foo"));
    assert!(!matches(Shape::Exact("foo"), "foo2"));
}

#[test]
fn covers_what_it_matches_a_superset_of() {
    let acme = scope("acme");
    let other = scope("other");
    let images = namespace("acme");
    assert!(covers(Shape::All, Shape::Scope(&acme)));
    assert!(covers(Shape::AnyScoped, Shape::Scope(&acme)));
    assert!(covers(Shape::AnyScoped, Shape::Exact("@acme/foo")));
    assert!(!covers(Shape::AnyScoped, Shape::Exact("foo")));
    assert!(!covers(Shape::AnyScoped, Shape::Namespace(&images)));
    assert!(covers(Shape::Scope(&acme), Shape::Exact("@acme/foo")));
    assert!(!covers(Shape::Scope(&acme), Shape::Scope(&other)));
    assert!(!covers(Shape::Scope(&acme), Shape::AnyScoped));
    assert!(covers(Shape::Namespace(&images), Shape::Exact("acme/app")));
    assert!(!covers(Shape::Namespace(&images), Shape::Scope(&acme)));
    assert!(!covers(Shape::Exact("foo"), Shape::All));
}
