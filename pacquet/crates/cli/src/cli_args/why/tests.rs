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
