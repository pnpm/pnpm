use pretty_assertions::assert_eq;
use serde_json::Value;

use super::render::{
    RenderDependentsOptions, render_dependents_json, render_dependents_parseable,
    render_dependents_tree,
};
use crate::cli_args::deps_tree::dependents::{DepField, DependentNode, DependentsTree};

fn tree(name: &str, version: &str, dependents: Vec<DependentNode>) -> DependentsTree {
    DependentsTree {
        name: name.to_string(),
        display_name: None,
        version: version.to_string(),
        path: None,
        peers_suffix_hash: None,
        dependents,
        search_message: None,
    }
}

fn importer(name: &str, version: &str, dep_field: DepField) -> DependentNode {
    DependentNode {
        name: name.to_string(),
        display_name: None,
        version: version.to_string(),
        circular: false,
        peers_suffix_hash: None,
        deduped: false,
        dep_field: Some(dep_field),
        dependents: None,
    }
}

fn package(name: &str, version: &str, dependents: Vec<DependentNode>) -> DependentNode {
    DependentNode {
        dep_field: None,
        dependents: Some(dependents),
        ..importer(name, version, DepField::Dependencies)
    }
}

/// Shared fixture: target ← mid-a ← root-project (2 levels of dependents).
fn deep_tree() -> Vec<DependentsTree> {
    vec![tree(
        "target",
        "1.0.0",
        vec![
            package(
                "mid-a",
                "2.0.0",
                vec![importer("root-project", "0.0.0", DepField::Dependencies)],
            ),
            importer("root-project", "0.0.0", DepField::DevDependencies),
        ],
    )]
}

fn opts(depth: Option<usize>) -> RenderDependentsOptions {
    RenderDependentsOptions { long: false, depth }
}

// Port of upstream's 'renders searchMessage below the root label' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn renders_search_message_below_the_root_label() {
    let results = vec![DependentsTree {
        search_message: Some("Matched by custom finder".to_string()),
        ..tree("foo", "1.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)])
    }];

    let output = render_dependents_tree(&results, &opts(None));
    let lines: Vec<&str> = output.split('\n').collect();

    eprintln!("output:\n{output}");
    assert!(lines[0].contains("foo@1.0.0"));
    assert!(lines.iter().any(|line| line.contains("Matched by custom finder")));
    assert!(lines.iter().any(|line| line.contains("my-project@0.0.0")));
}

// Port of upstream's 'does not render extra line when searchMessage is undefined' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn does_not_render_extra_line_when_search_message_is_undefined() {
    let results =
        vec![tree("foo", "1.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)])];

    let output = render_dependents_tree(&results, &opts(None));
    let lines: Vec<&str> = output.split('\n').collect();

    eprintln!("output:\n{output}");
    assert_eq!(lines[0], "foo@1.0.0");
    // Second line should be part of the tree, not a message.
    assert_ne!(lines[1], "");
    assert!(lines[1].contains("my-project"));
}

// Port of upstream's 'depth limits how deep the tree is rendered' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn depth_limits_how_deep_the_tree_is_rendered() {
    let with_depth = render_dependents_tree(&deep_tree(), &opts(Some(1)));
    let without_depth = render_dependents_tree(&deep_tree(), &opts(None));

    eprintln!("with depth:\n{with_depth}\n\nwithout depth:\n{without_depth}");

    // Without depth, root-project appears twice: once nested under
    // mid-a, once as direct dependent.
    let full_occurrences =
        without_depth.lines().filter(|line| line.contains("root-project@0.0.0")).count();
    assert_eq!(full_occurrences, 2);

    // With depth 1, mid-a's children are not expanded, so root-project
    // appears only once (as direct dependent).
    let limited_occurrences =
        with_depth.lines().filter(|line| line.contains("root-project@0.0.0")).count();
    assert_eq!(limited_occurrences, 1);
    assert!(with_depth.contains("mid-a@2.0.0"));
}

// Port of upstream's 'renders displayName instead of name when provided' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn renders_display_name_instead_of_name_when_provided() {
    let results = vec![DependentsTree {
        display_name: Some("my-component".to_string()),
        ..tree(
            "foo",
            "1.0.0",
            vec![DependentNode {
                display_name: Some("other-component".to_string()),
                ..package(
                    "bar",
                    "2.0.0",
                    vec![importer("my-project", "0.0.0", DepField::Dependencies)],
                )
            }],
        )
    }];

    let output = render_dependents_tree(&results, &opts(None));
    eprintln!("output:\n{output}");
    assert!(output.contains("my-component@1.0.0"));
    assert!(!output.contains("foo@1.0.0"));
    assert!(output.contains("other-component@2.0.0"));
    assert!(!output.contains("bar@2.0.0"));
    // Importer without displayName should still render its name.
    assert!(output.contains("my-project@0.0.0"));
}

// Port of upstream's 'falls back to name when displayName is undefined' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn falls_back_to_name_when_display_name_is_undefined() {
    let results =
        vec![tree("foo", "1.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)])];

    let output = render_dependents_tree(&results, &opts(None));
    eprintln!("output:\n{output}");
    assert!(output.contains("foo@1.0.0"));
}

// Port of upstream's 'renders package with no dependents and a searchMessage' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn renders_package_with_no_dependents_and_a_search_message() {
    let results = vec![DependentsTree {
        search_message: Some("Found via license check".to_string()),
        ..tree("bar", "2.0.0", vec![])
    }];

    let output = render_dependents_tree(&results, &opts(None));
    let lines: Vec<&str> = output.split('\n').collect();

    assert_eq!(lines[0], "bar@2.0.0");
    assert_eq!(lines[1], "Found via license check");
}

// Port of upstream's 'whySummary > single package, single version' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn why_summary_single_package_single_version() {
    let results =
        vec![tree("foo", "1.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)])];

    let output = render_dependents_tree(&results, &opts(None));
    eprintln!("output:\n{output}");
    assert!(output.contains("Found 1 version of foo"));
    assert!(!output.contains("instances"));
}

// Port of upstream's 'whySummary > single package, multiple versions' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn why_summary_single_package_multiple_versions() {
    let results = vec![
        tree("foo", "1.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)]),
        tree("foo", "2.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)]),
    ];

    let output = render_dependents_tree(&results, &opts(None));
    eprintln!("output:\n{output}");
    assert!(output.contains("Found 2 versions of foo"));
    assert!(!output.contains("instances"));
}

// Port of upstream's 'whySummary > single package, same version with multiple peer variants shows instance count' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn why_summary_single_package_same_version_with_multiple_peer_variants_shows_instance_count() {
    let results = vec![
        DependentsTree {
            peers_suffix_hash: Some("aaaa".to_string()),
            ..tree("foo", "1.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)])
        },
        DependentsTree {
            peers_suffix_hash: Some("bbbb".to_string()),
            ..tree("foo", "1.0.0", vec![importer("other", "0.0.0", DepField::Dependencies)])
        },
    ];

    let output = render_dependents_tree(&results, &opts(None));
    eprintln!("output:\n{output}");
    assert!(output.contains("Found 1 version, 2 instances of foo"));
}

// Port of upstream's 'whySummary > multiple different packages each get their own summary line' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn why_summary_multiple_different_packages_each_get_their_own_summary_line() {
    let results = vec![
        tree("foo", "1.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)]),
        tree("bar", "2.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)]),
        tree("bar", "3.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)]),
    ];

    let output = render_dependents_tree(&results, &opts(None));
    eprintln!("output:\n{output}");
    assert!(output.contains("Found 1 version of foo"));
    assert!(output.contains("Found 2 versions of bar"));
}

// Port of upstream's 'whySummary > summary uses displayName when provided' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn why_summary_uses_display_name_when_provided() {
    let results = vec![
        DependentsTree {
            display_name: Some("my-component".to_string()),
            ..tree("foo", "1.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)])
        },
        DependentsTree {
            display_name: Some("my-component".to_string()),
            ..tree("foo", "2.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)])
        },
    ];

    let output = render_dependents_tree(&results, &opts(None));
    eprintln!("output:\n{output}");
    assert!(output.contains("Found 2 versions of my-component"));
    assert!(!output.contains("Found 2 versions of foo"));
}

// Port of upstream's 'whySummary > empty results produce no summary' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn empty_results_produce_no_summary() {
    let output = render_dependents_tree(&[], &opts(None));
    assert_eq!(output, "");
}

// Port of upstream's 'renderDependentsJson > includes searchMessage in JSON output' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn includes_search_message_in_json_output() {
    let results = vec![DependentsTree {
        search_message: Some("Matched by custom finder".to_string()),
        ..tree("foo", "1.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)])
    }];

    let parsed: Value =
        serde_json::from_str(&render_dependents_json(&results, &opts(None))).expect("valid JSON");
    let trees = parsed.as_array().expect("JSON array");
    assert_eq!(trees.len(), 1);
    assert_eq!(trees[0]["searchMessage"], "Matched by custom finder");
}

// Port of upstream's 'renderDependentsJson > depth truncates dependents in JSON output' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn depth_truncates_dependents_in_json_output() {
    let parsed: Value = serde_json::from_str(&render_dependents_json(&deep_tree(), &opts(Some(1))))
        .expect("valid JSON");
    let trees = parsed.as_array().expect("JSON array");
    assert_eq!(trees.len(), 1);
    // Direct dependents (depth 0) should be present.
    let dependents = trees[0]["dependents"].as_array().expect("dependents array");
    assert_eq!(dependents.len(), 2);
    // mid-a should have its dependents stripped (depth 1 is beyond the limit).
    let mid_a = dependents.iter().find(|dep| dep["name"] == "mid-a").expect("mid-a present");
    dbg!(mid_a);
    assert!(mid_a.get("dependents").is_none());
    // root-project (direct dependent) should still be present.
    assert!(dependents.iter().any(|dep| dep["name"] == "root-project"));
}

// Port of upstream's 'renderDependentsJson > no depth option preserves full dependents in JSON output' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn no_depth_option_preserves_full_dependents_in_json_output() {
    let parsed: Value = serde_json::from_str(&render_dependents_json(&deep_tree(), &opts(None)))
        .expect("valid JSON");
    let dependents = parsed[0]["dependents"].as_array().expect("dependents array");
    let mid_a = dependents.iter().find(|dep| dep["name"] == "mid-a").expect("mid-a present");
    let mid_a_dependents = mid_a["dependents"].as_array().expect("mid-a dependents array");
    assert_eq!(mid_a_dependents.len(), 1);
    assert_eq!(mid_a_dependents[0]["name"], "root-project");
}

// Port of upstream's 'renderDependentsJson > includes displayName in JSON output' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn includes_display_name_in_json_output() {
    let results = vec![DependentsTree {
        display_name: Some("my-component".to_string()),
        ..tree(
            "foo",
            "1.0.0",
            vec![DependentNode {
                display_name: Some("other-component".to_string()),
                ..package(
                    "bar",
                    "2.0.0",
                    vec![importer("my-project", "0.0.0", DepField::Dependencies)],
                )
            }],
        )
    }];

    let parsed: Value =
        serde_json::from_str(&render_dependents_json(&results, &opts(None))).expect("valid JSON");
    assert_eq!(parsed[0]["name"], "foo");
    assert_eq!(parsed[0]["displayName"], "my-component");
    assert_eq!(parsed[0]["dependents"][0]["name"], "bar");
    assert_eq!(parsed[0]["dependents"][0]["displayName"], "other-component");
    // Nodes without displayName should not have the field.
    let importer_node = &parsed[0]["dependents"][0]["dependents"][0];
    dbg!(importer_node);
    assert!(importer_node.get("displayName").is_none());
}

// Port of upstream's 'renderDependentsJson > does not include searchMessage when undefined' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn does_not_include_search_message_when_undefined() {
    let results = vec![tree("foo", "1.0.0", vec![])];

    let parsed: Value =
        serde_json::from_str(&render_dependents_json(&results, &opts(None))).expect("valid JSON");
    dbg!(&parsed);
    assert!(parsed[0].get("searchMessage").is_none());
}

// Port of upstream's 'renderDependentsParseable > depth limits parseable output depth' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn depth_limits_parseable_output_depth() {
    let output = render_dependents_parseable(&deep_tree(), &opts(Some(1)));
    let lines: Vec<&str> = output.split('\n').collect();

    eprintln!("output:\n{output}");
    // With depth 1, mid-a cannot recurse further — it becomes a leaf.
    assert_eq!(lines.len(), 2);
    assert!(lines.contains(&"mid-a@2.0.0 > target@1.0.0"));
    assert!(lines.contains(&"root-project@0.0.0 > target@1.0.0"));
}

// Port of upstream's 'renderDependentsParseable > no depth option renders full paths in parseable output' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn no_depth_option_renders_full_paths_in_parseable_output() {
    let output = render_dependents_parseable(&deep_tree(), &opts(None));
    let lines: Vec<&str> = output.split('\n').collect();

    eprintln!("output:\n{output}");
    // Without depth limit, mid-a is expanded to root-project.
    assert_eq!(lines.len(), 2);
    assert!(lines.contains(&"root-project@0.0.0 > mid-a@2.0.0 > target@1.0.0"));
    assert!(lines.contains(&"root-project@0.0.0 > target@1.0.0"));
}

// Port of upstream's 'renderDependentsParseable > uses displayName in parseable output' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn uses_display_name_in_parseable_output() {
    let results = vec![DependentsTree {
        display_name: Some("my-component".to_string()),
        ..tree(
            "foo",
            "1.0.0",
            vec![DependentNode {
                display_name: Some("other-component".to_string()),
                ..package(
                    "bar",
                    "2.0.0",
                    vec![importer("my-project", "0.0.0", DepField::Dependencies)],
                )
            }],
        )
    }];

    let output = render_dependents_parseable(&results, &opts(None));
    assert_eq!(output, "my-project@0.0.0 > other-component@2.0.0 > my-component@1.0.0");
}

// Port of upstream's 'renderDependentsParseable > renders parseable output with searchMessage result' (deps/inspection/list/test/renderDependentsTree.test.ts).
#[test]
fn renders_parseable_output_with_search_message_result() {
    let results = vec![DependentsTree {
        search_message: Some("Found via custom check".to_string()),
        ..tree("dep-a", "1.0.0", vec![importer("my-project", "0.0.0", DepField::Dependencies)])
    }];

    let output = render_dependents_parseable(&results, &opts(None));
    let lines: Vec<&str> = output.split('\n').collect();

    eprintln!("output:\n{output}");
    // Parseable output should still contain the path.
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("dep-a@1.0.0"));
    assert!(lines[0].contains("my-project@0.0.0"));
}
