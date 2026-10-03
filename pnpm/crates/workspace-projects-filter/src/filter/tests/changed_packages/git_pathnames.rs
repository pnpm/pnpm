use super::{commit_all, diff_selector, git, graph_of, init_repo, node_at, selected, touch};
use crate::{
    filter::FilterWorkspaceProjectsOptions,
    parse_project_selector::{DependencyTraversal, ProjectSelector},
};
use indexmap::IndexMap;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

#[test]
fn unicode_and_space_directories_keep_their_owners() {
    assert_directory_owners(&["ascii", "with spaces", "\u{4e2d}\u{6587}", "\u{d55c}\u{ae00}"]);
}

#[test]
fn nested_git_paths_match_native_project_paths() {
    // On Windows, exercise Git's '/' paths against native PathBuf keys.
    let directory = Path::new("packages").join("\u{4e2d}\u{6587}").join("leaf with spaces");
    assert_directory_owners(&[directory.to_str().expect("UTF-8 directory")]);
}

#[test]
#[cfg_attr(not(unix), ignore = "requires POSIX quote, tab, newline, and trailing-space paths")]
fn posix_pathname_boundaries_keep_their_owners() {
    assert_directory_owners(&[
        r#""quoted""#,
        "\tleading-tab",
        "trailing-tab\t",
        "\nleading-newline",
        "trailing-newline\n",
        " leading and trailing spaces ",
    ]);
}

fn assert_directory_owners(dir_names: &[&str]) {
    let workspace = TempDir::new().expect("create tempdir");
    let workspace_dir = workspace.path();
    init_repo(workspace_dir);
    git(workspace_dir, &["config", "--local", "core.quotePath", "true"]);
    let dirs: Vec<_> = dir_names
        .iter()
        .map(|name| workspace_dir.join(name))
        .collect();
    let unchanged = workspace_dir.join("unchanged");
    touch(&unchanged.join("index.js"));
    for dir in &dirs {
        touch(&dir.join("index.js"));
        touch(&dir.join("second.js"));
    }
    commit_all(workspace_dir);
    let mut project_dirs = vec![workspace_dir, unchanged.as_path()];
    project_dirs.extend(dirs.iter().map(PathBuf::as_path));
    let graph = graph_of(&project_dirs);
    let opts = FilterWorkspaceProjectsOptions {
        workspace_dir: workspace_dir.to_path_buf(),
        ..Default::default()
    };
    assert_eq!(dbg!(selected(&graph, &[diff_selector("HEAD")], &opts)), Vec::<String>::new());

    for dir in &dirs {
        fs::write(dir.join("index.js"), "changed").expect("change tracked file");
        fs::write(dir.join("second.js"), "changed").expect("change second tracked file");
    }
    let expected: Vec<_> = dirs
        .iter()
        .map(|dir| dir.to_string_lossy().into_owned())
        .collect();
    for since in ["HEAD", "HEAD~1"] {
        if since == "HEAD~1" {
            commit_all(workspace_dir);
        }
        let actual = selected(&graph, &[diff_selector(since)], &opts);
        assert_eq!(dbg!(actual), expected, "directories: {dir_names:?}, since: {since}");
    }
    assert_eq!(dbg!(selected(&graph, &[diff_selector("HEAD")], &opts)), Vec::<String>::new());
}

#[test]
fn literal_unicode_and_space_filename_patterns_are_respected() {
    for filename in ["test.js", "test file.js", "\u{68c0}\u{67e5}.js", "\u{d55c}\u{ae00}.js"] {
        assert_literal_filename_patterns(filename);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "requires POSIX quote, tab, newline, and trailing-space paths")]
fn literal_posix_filename_patterns_are_respected() {
    for filename in [
        r#""quoted.js""#,
        "tab\tfile.js",
        "line\nfile.js",
        "file.js\n",
        "file.js\r",
        " leading and trailing spaces.js ",
    ] {
        assert_literal_filename_patterns(filename);
    }
}

fn assert_literal_filename_patterns(filename: &str) {
    let workspace = TempDir::new().expect("create tempdir");
    let workspace_dir = workspace.path();
    init_repo(workspace_dir);
    git(workspace_dir, &["config", "--local", "core.quotePath", "true"]);
    let leaf = workspace_dir.join("leaf");
    let consumer = workspace_dir.join("consumer");
    touch(&leaf.join(filename));
    touch(&leaf.join("source.js"));
    touch(&consumer.join("index.js"));
    commit_all(workspace_dir);
    let graph = IndexMap::from([
        node_at(workspace_dir, "root", &[]),
        node_at(&leaf, "leaf", &[]),
        node_at(&consumer, "consumer", &[&leaf]),
    ]);
    let path_of = |dir: &Path| dir.to_string_lossy().into_owned();
    let pattern = format!("**/{filename}");
    let opts = FilterWorkspaceProjectsOptions {
        workspace_dir: workspace_dir.to_path_buf(),
        ..Default::default()
    };
    let with_dependents = |since| ProjectSelector {
        traversal: DependencyTraversal { include_dependents: true, ..Default::default() },
        ..diff_selector(since)
    };
    fs::write(leaf.join(filename), "changed").expect("change tracked file");

    for since in ["HEAD", "HEAD~1"] {
        if since == "HEAD~1" {
            commit_all(workspace_dir);
        }
        assert_eq!(
            dbg!(selected(&graph, &[diff_selector(since)], &opts)),
            [path_of(&leaf)],
            "filename: {filename:?}, since: {since}",
        );
        for (test_pattern, changed_files_ignore_pattern, expected) in [
            (Vec::new(), Vec::new(), vec![path_of(&leaf), path_of(&consumer)]),
            (vec![pattern.clone()], Vec::new(), vec![path_of(&leaf)]),
            (Vec::new(), vec![pattern.clone()], Vec::new()),
        ] {
            let options = FilterWorkspaceProjectsOptions {
                test_pattern,
                changed_files_ignore_pattern,
                ..opts.clone()
            };
            let actual = selected(&graph, &[with_dependents(since)], &options);
            assert_eq!(
                dbg!(actual),
                expected,
                "filename: {filename:?}, since: {since}, test: {:?}, ignore: {:?}",
                options.test_pattern,
                options.changed_files_ignore_pattern,
            );
        }
    }

    fs::write(leaf.join("source.js"), "changed").expect("change nonmatching source file");
    assert_eq!(
        dbg!(selected(
            &graph,
            &[with_dependents("HEAD~1")],
            &FilterWorkspaceProjectsOptions { test_pattern: vec![pattern.clone()], ..opts.clone() },
        )),
        [path_of(&leaf), path_of(&consumer)],
        "a source change must include dependents alongside {filename:?}",
    );
    assert_eq!(
        dbg!(selected(
            &graph,
            &[diff_selector("HEAD~1")],
            &FilterWorkspaceProjectsOptions { changed_files_ignore_pattern: vec![pattern], ..opts },
        )),
        [path_of(&leaf)],
        "ignoring {filename:?} must not ignore the source change",
    );
}
