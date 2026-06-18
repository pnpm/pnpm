use super::*;

#[test]
fn test_dep_field_name() {
    assert_eq!(dep_field_name(DependencyGroup::Prod), "dependencies");
    assert_eq!(dep_field_name(DependencyGroup::Dev), "devDependencies");
    assert_eq!(dep_field_name(DependencyGroup::Optional), "optionalDependencies");
    assert_eq!(dep_field_name(DependencyGroup::Peer), "peerDependencies");
}

#[test]
fn test_render_tree_empty() {
    let results: Vec<WhyResult> = vec![];
    assert_eq!(render_tree(&results, None), "");
}

#[test]
fn test_render_tree_no_dependents() {
    let results = vec![WhyResult {
        name: "lodash".to_string(),
        version: "4.17.21".to_string(),
        dependents: vec![],
    }];
    let output = render_tree(&results, None);
    assert!(output.contains("lodash@4.17.21"));
}

#[test]
fn test_render_tree_with_dependents() {
    let results = vec![WhyResult {
        name: "lodash".to_string(),
        version: "4.17.21".to_string(),
        dependents: vec![DependentNode {
            name: "express".to_string(),
            version: "4.18.2".to_string(),
            dep_field: None,
            dependents: vec![DependentNode {
                name: "project".to_string(),
                version: "0.0.0".to_string(),
                dep_field: None,
                dependents: vec![],
            }],
        }],
    }];
    let output = render_tree(&results, None);
    assert!(output.contains("lodash@4.17.21"));
    assert!(output.contains("express@4.18.2"));
    assert!(output.contains("project@0.0.0"));
}

#[test]
fn test_render_tree_respects_depth() {
    let results = vec![WhyResult {
        name: "lodash".to_string(),
        version: "4.17.21".to_string(),
        dependents: vec![DependentNode {
            name: "express".to_string(),
            version: "4.18.2".to_string(),
            dep_field: None,
            dependents: vec![DependentNode {
                name: "project".to_string(),
                version: "0.0.0".to_string(),
                dep_field: None,
                dependents: vec![],
            }],
        }],
    }];
    let output = render_tree(&results, Some(1));
    assert!(output.contains("express@4.18.2"));
    assert!(!output.contains("project@0.0.0"));
}

#[test]
fn test_normalize_path_simple() {
    assert_eq!(normalize_path("packages/a", "../b"), Some("packages/b".to_string()));
}

#[test]
fn test_normalize_path_nested() {
    assert_eq!(normalize_path("packages/a", "../../other"), Some("other".to_string()));
}

#[test]
fn test_normalize_path_dot() {
    assert_eq!(normalize_path("packages/a", "./sibling"), Some("packages/a/sibling".to_string()));
}

#[test]
fn test_normalize_path_empty() {
    assert_eq!(normalize_path("packages/a", ""), Some("packages/a".to_string()));
}

#[test]
fn test_normalize_path_too_many_parents() {
    assert_eq!(normalize_path("a", "../../b"), None);
}
