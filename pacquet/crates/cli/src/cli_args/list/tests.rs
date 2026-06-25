use pretty_assertions::assert_eq;
use serde_json::Value;

use super::{
    DepNode, LocalTreeRoot, PkgNameVerPeer, dep_to_json, get_peer_set, glob_match, matches_params,
    name_at_version, print_label, read_root_manifest, render_local_json, render_local_parseable,
    render_local_tree, sort_deps,
};

#[test]
fn glob_match_exact() {
    assert!(glob_match("foo", "foo"));
    assert!(!glob_match("foo", "bar"));
}

#[test]
fn glob_match_wildcard_suffix() {
    assert!(glob_match("foo*", "foobar"));
    assert!(glob_match("foo*", "foo"));
    assert!(!glob_match("foo*", "f"));
}

#[test]
fn glob_match_wildcard_prefix() {
    assert!(glob_match("*bar", "foobar"));
    assert!(glob_match("*bar", "bar"));
    assert!(!glob_match("*bar", "barbaz"));
}

#[test]
fn glob_match_wildcard_infix() {
    assert!(glob_match("f*o", "foo"));
    assert!(glob_match("f*o", "f_o"));
    assert!(glob_match("f*o", "fooo"));
    assert!(!glob_match("f*o", "far"));
}

#[test]
fn glob_match_scoped_package() {
    assert!(glob_match("@scope/*", "@scope/foo"));
    assert!(!glob_match("@scope/*", "@other/foo"));
}

#[test]
fn matches_params_empty() {
    assert!(matches_params(&[], "anything"));
}

#[test]
fn matches_params_with_patterns() {
    assert!(matches_params(&["foo".to_string()], "foo"));
    assert!(!matches_params(&["foo".to_string()], "bar"));
    assert!(matches_params(&["foo*".to_string()], "foobar"));
}

#[test]
fn matches_params_multiple_patterns() {
    assert!(matches_params(&["foo".to_string(), "bar".to_string()], "bar"));
    assert!(!matches_params(&["foo".to_string(), "bar".to_string()], "baz"));
}

#[test]
fn sort_deps_by_name() {
    let mut deps = vec![
        DepNode {
            alias: "z-pkg".into(),
            name: "z-pkg".into(),
            version: "1.0.0".into(),
            path: String::new(),
            is_peer: false,
            is_dev: false,
            is_optional: false,
            dependencies: vec![],
        },
        DepNode {
            alias: "a-pkg".into(),
            name: "a-pkg".into(),
            version: "2.0.0".into(),
            path: String::new(),
            is_peer: false,
            is_dev: false,
            is_optional: false,
            dependencies: vec![],
        },
    ];
    sort_deps(&mut deps);
    assert_eq!(deps[0].name, "a-pkg");
    assert_eq!(deps[1].name, "z-pkg");
}

#[test]
fn sort_deps_recursive() {
    let mut deps = vec![DepNode {
        alias: "outer".into(),
        name: "outer".into(),
        version: "1.0.0".into(),
        path: String::new(),
        is_peer: false,
        is_dev: false,
        is_optional: false,
        dependencies: vec![
            DepNode {
                alias: "z-inner".into(),
                name: "z-inner".into(),
                version: "0.1.0".into(),
                path: String::new(),
                is_peer: false,
                is_dev: false,
                is_optional: false,
                dependencies: vec![],
            },
            DepNode {
                alias: "a-inner".into(),
                name: "a-inner".into(),
                version: "0.2.0".into(),
                path: String::new(),
                is_peer: false,
                is_dev: false,
                is_optional: false,
                dependencies: vec![],
            },
        ],
    }];
    sort_deps(&mut deps);
    assert_eq!(deps[0].dependencies[0].name, "a-inner");
    assert_eq!(deps[0].dependencies[1].name, "z-inner");
}

#[test]
fn read_root_manifest_with_all_fields() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let manifest = dir.path().join("package.json");
    std::fs::write(&manifest, r#"{"name":"test-pkg","version":"1.0.0","private":true}"#)
        .expect("write manifest");
    let (name, version, private) = read_root_manifest(&manifest).unwrap();
    assert_eq!(name, Some("test-pkg".into()));
    assert_eq!(version, Some("1.0.0".into()));
    assert_eq!(private, Some(true));
}

#[test]
fn read_root_manifest_missing_file() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let manifest = dir.path().join("nonexistent.json");
    let err = read_root_manifest(&manifest).unwrap_err();
    assert!(err.to_string().contains("failed to read"));
}

#[test]
fn read_root_manifest_invalid_json() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let manifest = dir.path().join("package.json");
    std::fs::write(&manifest, "not json").expect("write manifest");
    let err = read_root_manifest(&manifest).unwrap_err();
    assert!(err.to_string().contains("failed to parse"));
}

#[test]
fn render_local_tree_shows_root_with_name() {
    let root = LocalTreeRoot {
        name: Some("my-pkg".into()),
        version: Some("1.0.0".into()),
        private: Some(false),
        path: "/tmp/project".into(),
        dependencies: vec![],
        dev_dependencies: vec![],
        optional_dependencies: vec![],
    };
    let output = render_local_tree(&root, false);
    assert!(output.contains("my-pkg"));
}

#[test]
fn render_local_tree_shows_private() {
    let root = LocalTreeRoot {
        name: Some("my-pkg".into()),
        version: Some("1.0.0".into()),
        private: Some(true),
        path: "/tmp/project".into(),
        dependencies: vec![],
        dev_dependencies: vec![],
        optional_dependencies: vec![],
    };
    let output = render_local_tree(&root, false);
    assert!(output.contains("PRIVATE"));
}

#[test]
fn render_local_tree_with_deps() {
    let root = LocalTreeRoot {
        name: Some("root".into()),
        version: Some("0.0.0".into()),
        private: Some(false),
        path: "/project".into(),
        dependencies: vec![DepNode {
            alias: "foo".into(),
            name: "foo".into(),
            version: "1.0.0".into(),
            path: ".pnpm/foo@1.0.0/node_modules/foo".into(),
            is_peer: false,
            is_dev: false,
            is_optional: false,
            dependencies: vec![],
        }],
        dev_dependencies: vec![],
        optional_dependencies: vec![],
    };
    let output = render_local_tree(&root, false);
    assert!(output.contains("foo"));
    assert!(output.contains("1.0.0"));
}

#[test]
fn render_local_tree_empty_returns_root_line() {
    let root = LocalTreeRoot {
        name: Some("lone".into()),
        version: Some("0.0.0".into()),
        private: Some(false),
        path: "/nowhere".into(),
        dependencies: vec![],
        dev_dependencies: vec![],
        optional_dependencies: vec![],
    };
    let output = render_local_tree(&root, false);
    assert!(output.contains("lone"));
    assert!(output.contains("nowhere"));
}

#[test]
fn render_local_json_basic() {
    let root = LocalTreeRoot {
        name: Some("pkg".into()),
        version: Some("2.0.0".into()),
        private: Some(false),
        path: "/p".into(),
        dependencies: vec![DepNode {
            alias: "dep".into(),
            name: "dep".into(),
            version: "1.0.0".into(),
            path: ".pnpm/dep@1.0.0/node_modules/dep".into(),
            is_peer: false,
            is_dev: false,
            is_optional: false,
            dependencies: vec![],
        }],
        dev_dependencies: vec![],
        optional_dependencies: vec![],
    };
    let output = render_local_json(&root);
    assert!(output.contains("pkg"));
    assert!(output.contains("2.0.0"));
    assert!(output.contains("dep"));
    let parsed: Value = serde_json::from_str(&output).expect("valid json");
    assert!(parsed.is_array());
    assert_eq!(parsed[0]["name"], "pkg");
}

#[test]
fn render_local_json_private() {
    let root = LocalTreeRoot {
        name: Some("secret".into()),
        version: None,
        private: Some(true),
        path: "/hidden".into(),
        dependencies: vec![],
        dev_dependencies: vec![],
        optional_dependencies: vec![],
    };
    let output = render_local_json(&root);
    let parsed: Value = serde_json::from_str(&output).expect("valid json");
    assert_eq!(parsed[0]["private"], true);
}

#[test]
fn render_local_parseable_flat() {
    let root = LocalTreeRoot {
        name: Some("root".into()),
        version: Some("1.0.0".into()),
        private: Some(false),
        path: "/r".into(),
        dependencies: vec![DepNode {
            alias: "a".into(),
            name: "a".into(),
            version: "0.1.0".into(),
            path: ".pnpm/a@0.1.0/node_modules/a".into(),
            is_peer: false,
            is_dev: false,
            is_optional: false,
            dependencies: vec![],
        }],
        dev_dependencies: vec![],
        optional_dependencies: vec![],
    };
    let output = render_local_parseable(&root, false);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "/r");
    assert!(lines[1].contains(".pnpm/a@0.1.0"));
}

#[test]
fn render_local_parseable_long() {
    let root = LocalTreeRoot {
        name: Some("test".into()),
        version: Some("3.0.0".into()),
        private: Some(false),
        path: "/t".into(),
        dependencies: vec![DepNode {
            alias: "b".into(),
            name: "b".into(),
            version: "2.0.0".into(),
            path: ".pnpm/b@2.0.0/node_modules/b".into(),
            is_peer: false,
            is_dev: false,
            is_optional: false,
            dependencies: vec![],
        }],
        dev_dependencies: vec![],
        optional_dependencies: vec![],
    };
    let output = render_local_parseable(&root, true);
    assert!(output.contains(":test@3.0.0"));
    assert!(output.contains("b@2.0.0"));
}

#[test]
fn dep_to_json_nested() {
    let dep = DepNode {
        alias: "parent".into(),
        name: "parent".into(),
        version: "1.0.0".into(),
        path: ".pnpm/parent@1.0.0/node_modules/parent".into(),
        is_peer: false,
        is_dev: false,
        is_optional: false,
        dependencies: vec![DepNode {
            alias: "child".into(),
            name: "child".into(),
            version: "0.1.0".into(),
            path: ".pnpm/child@0.1.0/node_modules/child".into(),
            is_peer: false,
            is_dev: false,
            is_optional: false,
            dependencies: vec![],
        }],
    };
    let json = dep_to_json(&dep);
    assert_eq!(json["from"], "parent");
    assert_eq!(json["version"], "1.0.0");
    assert!(json.get("dependencies").is_some());
}

#[test]
fn get_peer_set_empty_when_no_packages() {
    let key = "pkg@1.0.0".parse::<PkgNameVerPeer>().expect("parse pkg@1.0.0");
    let set = get_peer_set(&key, None);
    assert!(set.is_empty());
}

#[test]
fn name_at_version_with_and_without() {
    assert_eq!(name_at_version("pkg", ""), "pkg");
    let with_v = name_at_version("pkg", "1.0.0");
    assert!(with_v.starts_with("pkg"));
}

#[test]
fn print_label_alias_match() {
    let dep = DepNode {
        alias: "mypkg".into(),
        name: "mypkg".into(),
        version: "1.0.0".into(),
        path: String::new(),
        is_peer: false,
        is_dev: false,
        is_optional: false,
        dependencies: vec![],
    };
    let label = print_label(&dep, &|text| text.to_string());
    assert!(label.starts_with("mypkg"));
    assert!(label.contains("1.0.0"));
}

#[test]
fn print_label_alias_different() {
    let dep = DepNode {
        alias: "my-alias".into(),
        name: "real-name".into(),
        version: "2.0.0".into(),
        path: String::new(),
        is_peer: false,
        is_dev: false,
        is_optional: false,
        dependencies: vec![],
    };
    let label = print_label(&dep, &|text| text.to_string());
    assert!(label.starts_with("my-alias"));
    assert!(label.contains("real-name"));
}

#[test]
fn print_label_alias_with_version_containing_at() {
    let dep = DepNode {
        alias: "aliased".into(),
        name: "target".into(),
        version: "npm:@scope/pkg@1.0.0".into(),
        path: String::new(),
        is_peer: false,
        is_dev: false,
        is_optional: false,
        dependencies: vec![],
    };
    let label = print_label(&dep, &|text| text.to_string());
    assert!(label.starts_with("aliased"));
    assert!(label.contains("npm:@scope/pkg@1.0.0"));
}
