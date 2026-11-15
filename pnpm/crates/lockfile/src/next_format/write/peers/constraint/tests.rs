use super::ProviderConstraint;
use crate::{BoundProviders, PeerBindings};

#[test]
fn recorded_link_binding_matches_the_alias_and_path() {
    let providers =
        BoundProviders::of(&PeerBindings::Explicit(vec!["provider@packages+provider".to_string()]));
    let constraint = ProviderConstraint::new(&"provider".parse().unwrap(), None, Some(&providers));
    assert!(constraint.matches(&"link:packages/provider".parse().unwrap()));
    assert!(!constraint.matches(&"link:packages/other".parse().unwrap()));
    assert!(!constraint.matches(&"1.0.0".parse().unwrap()));
    let other = ProviderConstraint::new(&"other".parse().unwrap(), None, Some(&providers));
    assert!(!other.matches(&"link:packages/provider".parse().unwrap()));
}
