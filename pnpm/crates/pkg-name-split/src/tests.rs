use super::{
    ScopeSplit,
    SplitNameSuffixError,
    SplitScopeError,
    split_name_suffix,
    split_scope,
};
use pretty_assertions::assert_eq;

#[test]
fn splits_a_scoped_name_at_its_first_slash() {
    let Ok(ScopeSplit::Scoped { scope, bare }) = split_scope("@types/node/extra") else {
        panic!("a scoped name splits");
    };
    assert_eq!((scope, bare), ("types", "node/extra"));
}

#[test]
fn reports_names_without_a_scope_split() {
    assert!(matches!(split_scope("lodash"), Ok(ScopeSplit::Unscoped)));
    assert!(matches!(split_scope("@types"), Err(SplitScopeError::MissingBare)));
    assert!(matches!(split_scope(""), Err(SplitScopeError::Empty)));
}

#[test]
fn splits_a_key_at_the_first_at_after_the_first_character() {
    assert_eq!(split_name_suffix("ts-node@10.9.1").ok(), Some(("ts-node", "10.9.1")));
    assert_eq!(split_name_suffix("@types/node@18.7.19").ok(), Some(("@types/node", "18.7.19")));
    assert_eq!(
        split_name_suffix("react-json-view@1.21.3(@types/react@17.0.49)").ok(),
        Some(("react-json-view", "1.21.3(@types/react@17.0.49)")),
    );
    assert_eq!(
        split_name_suffix("@types/react-dom@17.0.2(@types/react@17.0.49)").ok(),
        Some(("@types/react-dom", "17.0.2(@types/react@17.0.49)")),
    );
}

#[test]
fn keeps_multibyte_names_on_character_boundaries() {
    assert_eq!(split_name_suffix("@é/ñ@1.0.0").ok(), Some(("@é/ñ", "1.0.0")));
}

#[test]
fn reports_keys_without_a_name_and_suffix_split() {
    assert!(matches!(split_name_suffix(""), Err(SplitNameSuffixError::EmptyInput)));
    assert!(matches!(split_name_suffix("@types/node"), Err(SplitNameSuffixError::MissingSuffix)));
    assert!(matches!(split_name_suffix("lodash"), Err(SplitNameSuffixError::MissingSuffix)));
    assert_eq!(split_name_suffix("@@1").ok(), Some(("@", "1")));
}
