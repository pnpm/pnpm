use pretty_assertions::assert_eq;
use text_block_macros::text_block;

use super::{
    RecursionLimit, parse_depth,
    render::{
        ProjectHierarchy, RenderParseableOptions, RenderTreeOptions, render_parseable, render_tree,
    },
};
use crate::cli_args::deps_tree::{DependencyNode, build::DependenciesHierarchy};

#[test]
fn parse_depth_accepts_infinity_and_minus_one() {
    assert_eq!(parse_depth("Infinity").unwrap(), RecursionLimit::Unlimited);
    assert_eq!(parse_depth("-1").unwrap(), RecursionLimit::ProjectsOnly);
    assert_eq!(parse_depth("0").unwrap(), RecursionLimit::Levels(0));
    assert_eq!(parse_depth("3").unwrap(), RecursionLimit::Levels(3));
    assert!(parse_depth("-2").is_err());
}

fn dep(alias: &str, name: &str, version: &str, path: &str) -> DependencyNode {
    DependencyNode {
        alias: alias.to_string(),
        name: name.to_string(),
        version: version.to_string(),
        path: path.to_string(),
        ..DependencyNode::default()
    }
}

fn project(
    name: &str,
    version: &str,
    path: &str,
    hierarchy: DependenciesHierarchy,
) -> ProjectHierarchy {
    ProjectHierarchy {
        name: Some(name.to_string()),
        version: Some(version.to_string()),
        private: false,
        path: path.to_string(),
        hierarchy,
    }
}

fn tree_opts(always_print_root_package: bool, show_extraneous: bool) -> RenderTreeOptions {
    RenderTreeOptions {
        always_print_root_package,
        depth_above_projects_only: true,
        long: false,
        show_extraneous,
        show_summary: false,
    }
}

fn parseable_opts(long: bool) -> RenderParseableOptions {
    RenderParseableOptions { long, always_print_root_package: false }
}

// Port of upstream's 'print empty' (deps/inspection/list/test/index.ts).
#[test]
fn print_empty() {
    let projects = vec![project("empty", "1.0.0", "/empty", DependenciesHierarchy::default())];

    let output = render_tree(&projects, &tree_opts(true, false));
    assert_eq!(
        output,
        "Legend: production dependency, optional only, dev only\n\nempty@1.0.0 /empty",
    );
}

// Port of upstream's "don't print empty" (deps/inspection/list/test/index.ts).
#[test]
fn dont_print_empty() {
    let projects = vec![project("empty", "1.0.0", "/empty", DependenciesHierarchy::default())];

    let output = render_tree(&projects, &tree_opts(false, false));
    assert_eq!(output, "");
}

// Port of upstream's 'unsaved dependencies are marked' (deps/inspection/list/test/index.ts).
#[test]
fn unsaved_dependencies_are_marked() {
    let projects = vec![project(
        "fixture",
        "1.0.0",
        "/fixture",
        DependenciesHierarchy {
            unsaved_dependencies: vec![dep("foo", "foo", "1.0.0", "")],
            ..DependenciesHierarchy::default()
        },
    )];

    let output = render_tree(&projects, &tree_opts(false, true));
    assert_eq!(
        output,
        text_block! {
            "Legend: production dependency, optional only, dev only"
            ""
            "fixture@1.0.0 /fixture"
            "│"
            "│   not saved (you should add these dependencies to package.json if you need them):"
            "└── foo@1.0.0"
        },
    );
}

// Port of upstream's 'list with many dependencies' (deps/inspection/list/test/index.ts).
#[test]
fn list_with_many_dependencies() {
    let projects = vec![project(
        "fixture",
        "1.0.0",
        "/fixture",
        DependenciesHierarchy {
            dependencies: ["a", "b", "c", "d", "e", "f", "g", "h", "i", "k", "l"]
                .iter()
                .map(|name| dep(name, name, "1.0.0", ""))
                .collect(),
            ..DependenciesHierarchy::default()
        },
    )];

    let output = render_tree(&projects, &tree_opts(false, false));
    assert_eq!(
        output,
        text_block! {
            "Legend: production dependency, optional only, dev only"
            ""
            "fixture@1.0.0 /fixture"
            "│"
            "│   dependencies:"
            "├── a@1.0.0"
            "├── b@1.0.0"
            "├── c@1.0.0"
            "├── d@1.0.0"
            "├── e@1.0.0"
            "├── f@1.0.0"
            "├── g@1.0.0"
            "├── h@1.0.0"
            "├── i@1.0.0"
            "├── k@1.0.0"
            "└── l@1.0.0"
        },
    );
}

// Port of upstream's 'sort list items' (deps/inspection/list/test/index.ts).
#[test]
fn sort_list_items() {
    let projects = vec![project(
        "fixture",
        "1.0.0",
        "/fixture",
        DependenciesHierarchy {
            dependencies: vec![DependencyNode {
                dependencies: vec![dep("qar", "qar", "1.0.0", ""), dep("bar", "bar", "1.0.0", "")],
                ..dep("foo", "foo", "1.0.0", "")
            }],
            ..DependenciesHierarchy::default()
        },
    )];

    let output = render_tree(&projects, &tree_opts(false, false));
    assert_eq!(
        output,
        text_block! {
            "Legend: production dependency, optional only, dev only"
            ""
            "fixture@1.0.0 /fixture"
            "│"
            "│   dependencies:"
            "└─┬ foo@1.0.0"
            "  ├── bar@1.0.0"
            "  └── qar@1.0.0"
        },
    );
}

// Port of upstream's 'renderTree displays npm: protocol for aliased packages' (deps/inspection/list/test/index.ts).
#[test]
fn render_tree_displays_npm_protocol_for_aliased_packages() {
    let projects = vec![project(
        "test-project",
        "1.0.0",
        "/test/path",
        DependenciesHierarchy {
            dependencies: vec![dep(
                "foo",
                "@pnpm.e2e/pkg-with-1-dep",
                "100.0.0",
                "/test/path/node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep",
            )],
            ..DependenciesHierarchy::default()
        },
    )];

    let output = render_tree(&projects, &tree_opts(false, false));
    eprintln!("output:\n{output}");
    assert!(output.contains("foo"));
    assert!(output.contains("npm:@pnpm.e2e/pkg-with-1-dep@100.0.0"));
}

// Port of upstream's 'renderTree displays file: protocol correctly for aliased packages' (deps/inspection/list/test/index.ts).
#[test]
fn render_tree_displays_file_protocol_correctly_for_aliased_packages() {
    let projects = vec![project(
        "test-project",
        "1.0.0",
        "/test/path",
        DependenciesHierarchy {
            dependencies: vec![dep(
                "my-alias",
                "my-local-pkg",
                "my-local-pkg@file:local-pkg",
                "/test/path/local-pkg",
            )],
            ..DependenciesHierarchy::default()
        },
    )];

    let output = render_tree(&projects, &tree_opts(false, false));
    eprintln!("output:\n{output}");
    assert!(output.contains("my-alias"));
    assert!(output.contains("my-local-pkg@file:local-pkg"));
}

// Port of upstream's 'renderParseable displays npm: protocol for aliased packages' (deps/inspection/list/test/index.ts).
#[test]
fn render_parseable_displays_npm_protocol_for_aliased_packages() {
    let projects = vec![project(
        "test-project",
        "1.0.0",
        "/test/path",
        DependenciesHierarchy {
            dependencies: vec![dep(
                "foo",
                "@pnpm.e2e/pkg-with-1-dep",
                "100.0.0",
                "/test/path/node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep",
            )],
            ..DependenciesHierarchy::default()
        },
    )];

    let output = render_parseable(&projects, &parseable_opts(true));
    eprintln!("output:\n{output}");
    assert!(output.contains("foo npm:@pnpm.e2e/pkg-with-1-dep@100.0.0"));
}

// Port of upstream's 'renderParseable displays file: protocol correctly for aliased packages' (deps/inspection/list/test/index.ts).
#[test]
fn render_parseable_displays_file_protocol_correctly_for_aliased_packages() {
    let projects = vec![project(
        "test-project",
        "1.0.0",
        "/test/path",
        DependenciesHierarchy {
            dependencies: vec![dep(
                "my-alias",
                "my-local-pkg",
                "my-local-pkg@file:local-pkg",
                "/test/path/local-pkg",
            )],
            ..DependenciesHierarchy::default()
        },
    )];

    let output = render_parseable(&projects, &parseable_opts(true));
    eprintln!("output:\n{output}");
    assert!(output.contains("my-alias my-local-pkg@file:local-pkg"));
}

// Port of upstream's 'renderParseable search: shared dep across packages is not duplicated' (deps/inspection/list/test/index.ts).
#[test]
fn render_parseable_search_shared_dep_across_packages_is_not_duplicated() {
    let shared_dep = || dep("@org/shared", "@org/shared", "1.0.0", "/workspace/packages/shared");
    let projects = vec![
        project(
            "pkg-a",
            "1.0.0",
            "/workspace/packages/pkg-a",
            DependenciesHierarchy {
                dependencies: vec![shared_dep()],
                ..DependenciesHierarchy::default()
            },
        ),
        project(
            "pkg-b",
            "1.0.0",
            "/workspace/packages/pkg-b",
            DependenciesHierarchy {
                dependencies: vec![shared_dep()],
                ..DependenciesHierarchy::default()
            },
        ),
        project(
            "@org/shared",
            "1.0.0",
            "/workspace/packages/shared",
            DependenciesHierarchy::default(),
        ),
    ];

    let output = render_parseable(&projects, &parseable_opts(false));
    let lines: Vec<&str> = output.split('\n').collect();

    eprintln!("output:\n{output}");
    assert!(lines.contains(&"/workspace/packages/pkg-a"));
    assert!(lines.contains(&"/workspace/packages/pkg-b"));
    assert!(lines.contains(&"/workspace/packages/shared"));
    assert_eq!(lines.iter().filter(|line| **line == "/workspace/packages/shared").count(), 1);
}

// Port of upstream's 'renderParseable search: packages unrelated to search are excluded' (deps/inspection/list/test/index.ts).
#[test]
fn render_parseable_search_packages_unrelated_to_search_are_excluded() {
    let projects = vec![
        project("root", "1.0.0", "/workspace", DependenciesHierarchy::default()),
        project(
            "pkg-a",
            "1.0.0",
            "/workspace/packages/pkg-a",
            DependenciesHierarchy {
                dependencies: vec![dep(
                    "@org/shared",
                    "@org/shared",
                    "1.0.0",
                    "/workspace/packages/shared",
                )],
                ..DependenciesHierarchy::default()
            },
        ),
        project(
            "unrelated",
            "1.0.0",
            "/workspace/packages/unrelated",
            DependenciesHierarchy::default(),
        ),
    ];

    let output = render_parseable(&projects, &parseable_opts(false));
    let lines: Vec<&str> = output.split('\n').collect();

    eprintln!("output:\n{output}");
    assert!(lines.contains(&"/workspace/packages/pkg-a"));
    assert!(lines.contains(&"/workspace/packages/shared"));
    assert!(!lines.contains(&"/workspace"));
    assert!(!lines.contains(&"/workspace/packages/unrelated"));
}

// Port of upstream's 'renderParseable search long: shared dep across packages is not duplicated' (deps/inspection/list/test/index.ts).
#[test]
fn render_parseable_search_long_shared_dep_across_packages_is_not_duplicated() {
    let shared_dep =
        || dep("@org/shared", "@org/shared", "link:../shared", "/workspace/packages/shared");
    let projects = vec![
        project(
            "pkg-a",
            "1.0.0",
            "/workspace/packages/pkg-a",
            DependenciesHierarchy {
                dependencies: vec![shared_dep()],
                ..DependenciesHierarchy::default()
            },
        ),
        project(
            "pkg-b",
            "1.0.0",
            "/workspace/packages/pkg-b",
            DependenciesHierarchy {
                dependencies: vec![shared_dep()],
                ..DependenciesHierarchy::default()
            },
        ),
    ];

    let output = render_parseable(&projects, &parseable_opts(true));
    let lines: Vec<&str> = output.split('\n').collect();

    eprintln!("output:\n{output}");
    assert!(lines.contains(&"/workspace/packages/pkg-a:pkg-a@1.0.0"));
    assert!(lines.contains(&"/workspace/packages/pkg-b:pkg-b@1.0.0"));
    assert_eq!(
        lines.iter().filter(|line| line.starts_with("/workspace/packages/shared")).count(),
        1,
    );
}
