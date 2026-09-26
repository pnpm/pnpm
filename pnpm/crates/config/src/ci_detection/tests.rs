use super::{EnvVar, detect_ci};
use serde_json::Value;
use std::{cell::RefCell, collections::HashMap};

thread_local! {
    static ENV: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

struct FakeEnv;

impl EnvVar for FakeEnv {
    fn var(name: &str) -> Option<String> {
        ENV.with_borrow(|env| env.get(name).cloned())
    }
}

fn detect_ci_with(vars: &[(&str, &str)]) -> bool {
    ENV.set(
        vars.iter()
            .map(|&(name, value)| (name.to_string(), value.to_string()))
            .collect(),
    );
    detect_ci::<FakeEnv>()
}

#[test]
fn aws_codebuild_is_ci_without_the_ci_variable() {
    assert!(detect_ci_with(&[("CODEBUILD_BUILD_ARN", "arn:aws:codebuild:build/x")]));
}

#[test]
fn any_non_empty_ci_value_other_than_false_is_ci() {
    assert!(detect_ci_with(&[("CI", "0")]));
    assert!(detect_ci_with(&[("CI", "woodpecker")]));
    assert!(!detect_ci_with(&[("CI", "")]));
    assert!(!detect_ci_with(&[]));
}

#[test]
fn ci_false_overrides_vendor_variables() {
    assert!(!detect_ci_with(&[("CI", "false"), ("GITHUB_ACTIONS", "true")]));
    assert!(!detect_ci_with(&[("CI", "false"), ("CODEBUILD_BUILD_ARN", "x")]));
}

#[test]
fn empty_vendor_variables_are_not_ci() {
    assert!(!detect_ci_with(&[("CODEBUILD_BUILD_ARN", ""), ("TF_BUILD", "")]));
}

#[test]
fn heroku_is_detected_by_its_node_path() {
    assert!(detect_ci_with(&[("NODE", "/app/.heroku/node/bin/node")]));
    assert!(!detect_ci_with(&[("NODE", "/usr/bin/node")]));
}

/// Every vendor in the `ci-info` release pnpm 11 depends on must be
/// detected, so a `ci-info` upgrade that adds a vendor fails here until
/// the Rust list catches up.
#[test]
fn every_ci_info_vendor_is_detected() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../pnpm11/config/reader/node_modules/ci-info/vendors.json",
    );
    let vendors = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read {path} (run `pnpm install`): {error}"));
    let vendors: Vec<Value> = serde_json::from_str(&vendors).expect("parse vendors.json");
    assert!(!vendors.is_empty());
    for vendor in vendors {
        for vars in vendor_envs(&vendor["env"]) {
            let vars: Vec<(&str, &str)> = vars
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str()))
                .collect();
            assert!(detect_ci_with(&vars), "{} with {vars:?} must be CI", vendor["name"]);
        }
    }
}

fn vendor_envs(spec: &Value) -> Vec<Vec<(String, String)>> {
    match spec {
        Value::String(name) => vec![vec![(name.clone(), "1".to_string())]],
        Value::Array(specs) => {
            vec![
                specs
                    .iter()
                    .flat_map(|spec| vendor_envs(spec).swap_remove(0))
                    .collect(),
            ]
        }
        Value::Object(object) => {
            if let (Some(Value::String(name)), Some(Value::String(includes))) =
                (object.get("env"), object.get("includes"))
            {
                vec![vec![(name.clone(), includes.clone())]]
            } else if let Some(Value::Array(any)) = object.get("any") {
                any.iter()
                    .flat_map(vendor_envs)
                    .collect()
            } else {
                vec![
                    object
                        .iter()
                        .map(|(name, value)| {
                            (
                                name.clone(),
                                value
                                    .as_str()
                                    .expect("string env value")
                                    .to_string(),
                            )
                        })
                        .collect(),
                ]
            }
        }
        other => panic!("unexpected vendors.json env entry: {other}"),
    }
}
