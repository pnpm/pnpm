use super::{
    BuildTaskGraphOptions, ScheduleGraphAsyncOptions, ScheduleGraphOptions, ScheduleTasksOptions,
    SequenceTasksOptions, TaskCompletion, TaskCycle, TaskGraph, TaskKey, build_task_graph,
    is_serial_task_graph, render_task_graph_dry_run, resume_task_graph_from, reverse_task_graph,
    schedule_graph, schedule_graph_async, schedule_tasks, sequence_tasks, task_graph_to_json,
};
use indexmap::IndexMap;
use pnpm_config::TaskSettings;
use pnpm_reporter::LogEvent;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Condvar, Mutex},
};

const WORKSPACE_DIR: &str = "/workspace";

fn dir(name: &str) -> PathBuf {
    Path::new(WORKSPACE_DIR).join(name)
}

fn key(name: &str, task_name: &str) -> TaskKey {
    TaskKey { project: dir(name), task_name: task_name.to_string() }
}

fn drop_event(_: &LogEvent) {}

fn sequence(graph: &mut TaskGraph) -> Result<Vec<TaskKey>, TaskCycle> {
    sequence_tasks(
        graph,
        &SequenceTasksOptions {
            workspace_dir: Path::new(WORKSPACE_DIR),
            ignore_cycles: false,
            emit: drop_event,
        },
    )
}

struct FakeProject {
    dependencies: Vec<&'static str>,
    scripts: Vec<&'static str>,
}

fn project(dependencies: &[&'static str], scripts: &[&'static str]) -> FakeProject {
    FakeProject { dependencies: dependencies.to_vec(), scripts: scripts.to_vec() }
}

fn tasks(entries: &[(&str, Option<&[&str]>)]) -> IndexMap<String, TaskSettings> {
    entries
        .iter()
        .map(|(name, depends_on)| {
            (
                name.to_string(),
                TaskSettings {
                    depends_on: depends_on.map(|entries| {
                        entries.iter().map(std::string::ToString::to_string).collect()
                    }),
                    unknown: IndexMap::new(),
                },
            )
        })
        .collect()
}

fn build_graph(
    projects: &[(&'static str, FakeProject)],
    task_name: &str,
    task_settings: Option<&IndexMap<String, TaskSettings>>,
) -> TaskGraph {
    let project_dependencies: IndexMap<PathBuf, Vec<PathBuf>> = projects
        .iter()
        .map(|(name, project)| {
            (dir(name), project.dependencies.iter().map(|dependency| dir(dependency)).collect())
        })
        .collect();
    let scripts_by_dir: HashMap<PathBuf, Vec<String>> = projects
        .iter()
        .map(|(name, project)| {
            (dir(name), project.scripts.iter().map(std::string::ToString::to_string).collect())
        })
        .collect();
    build_task_graph(&BuildTaskGraphOptions {
        project_dependencies: &project_dependencies,
        select_scripts: |project: &Path, task_name: &str| {
            scripts_by_dir[project].iter().filter(|script| *script == task_name).cloned().collect()
        },
        task_name,
        tasks: task_settings,
    })
}

#[test]
fn unconfigured_task_depends_on_the_same_task_in_workspace_dependencies() {
    let graph = build_graph(
        &[("a", project(&["b"], &["build"])), ("b", project(&[], &["build"]))],
        "build",
        None,
    );

    assert_eq!(graph.len(), 2);
    assert_eq!(graph[&key("a", "build")].dependencies, vec![key("b", "build")]);
    assert!(graph[&key("b", "build")].dependencies.is_empty());
    assert!(graph[&key("a", "build")].requested);
}

#[test]
fn same_project_depends_on_entry_pulls_the_named_task_into_the_graph() {
    let settings = tasks(&[("build", Some(&["^build"])), ("test", Some(&["build"]))]);
    let graph = build_graph(
        &[("a", project(&["b"], &["build", "test"])), ("b", project(&[], &["build", "test"]))],
        "test",
        Some(&settings),
    );

    assert_eq!(graph.len(), 4);
    assert_eq!(graph[&key("a", "test")].dependencies, vec![key("a", "build")]);
    assert_eq!(graph[&key("a", "build")].dependencies, vec![key("b", "build")]);
    assert!(!graph[&key("a", "build")].requested);
    assert!(graph[&key("a", "test")].requested);
}

#[test]
fn explicitly_empty_depends_on_means_the_task_depends_on_nothing() {
    let settings = tasks(&[("lint", None)]);
    let graph = build_graph(
        &[("a", project(&["b"], &["lint"])), ("b", project(&[], &["lint"]))],
        "lint",
        Some(&settings),
    );

    assert!(graph[&key("a", "lint")].dependencies.is_empty());
}

#[test]
fn project_without_the_script_becomes_a_pass_through_node_that_keeps_the_chain() {
    let mut graph = build_graph(
        &[
            ("a", project(&["b"], &["build"])),
            ("b", project(&["c"], &[])),
            ("c", project(&[], &["build"])),
        ],
        "build",
        None,
    );

    let pass_through = &graph[&key("b", "build")];
    assert!(pass_through.scripts.is_empty());
    assert_eq!(pass_through.dependencies, vec![key("c", "build")]);
    assert_eq!(graph[&key("a", "build")].dependencies, vec![key("b", "build")]);

    let order = sequence(&mut graph).unwrap();
    assert_eq!(order, vec![key("c", "build"), key("b", "build"), key("a", "build")]);
}

#[test]
fn task_cycle_is_an_error_naming_the_participating_tasks() {
    let mut graph = build_graph(
        &[("a", project(&["b"], &["build"])), ("b", project(&["a"], &["build"]))],
        "build",
        None,
    );
    let error = sequence(&mut graph).unwrap_err();
    dbg!(&error.cycles);
    assert!(error.cycles.contains("a#build"));
    assert!(error.cycles.contains("b#build"));
}

#[test]
fn task_depending_on_itself_is_an_error() {
    let settings = tasks(&[("build", Some(&["build"]))]);
    let mut graph = build_graph(&[("a", project(&[], &["build"]))], "build", Some(&settings));
    assert!(sequence(&mut graph).is_err());
}

#[test]
fn reverse_task_graph_runs_dependents_before_dependencies() {
    let graph = build_graph(
        &[("a", project(&["b"], &["build"])), ("b", project(&[], &["build"]))],
        "build",
        None,
    );
    let reversed = reverse_task_graph(&graph);
    assert!(reversed[&key("a", "build")].dependencies.is_empty());
    assert_eq!(reversed[&key("b", "build")].dependencies, vec![key("a", "build")]);
}

#[test]
fn resume_drops_only_the_anchors_transitive_dependencies() {
    let graph = build_graph(
        &[
            ("a", project(&[], &["build"])),
            ("b", project(&["a"], &["build"])),
            ("c", project(&["b"], &["build"])),
            ("unrelated", project(&[], &["build"])),
        ],
        "build",
        None,
    );

    let resumed = resume_task_graph_from(graph, &dir("b"), "build");

    assert_eq!(resumed.len(), 3);
    assert!(!resumed.contains_key(&key("a", "build")));
    // The edge into the dropped dependency is treated as satisfied.
    assert!(resumed[&key("b", "build")].dependencies.is_empty());
    assert_eq!(resumed[&key("c", "build")].dependencies, vec![key("b", "build")]);
    assert!(resumed.contains_key(&key("unrelated", "build")));
}

#[test]
fn is_serial_tells_a_chain_from_a_graph_with_independent_tasks() {
    let mut chain = build_graph(
        &[
            ("a", project(&["b"], &["build"])),
            ("b", project(&["c"], &["build"])),
            ("c", project(&[], &["build"])),
        ],
        "build",
        None,
    );
    let sequenced = sequence(&mut chain).unwrap();
    assert!(is_serial_task_graph(&chain, &sequenced));

    let mut diamond = build_graph(
        &[
            ("a", project(&["b", "c"], &["build"])),
            ("b", project(&["d"], &["build"])),
            ("c", project(&["d"], &["build"])),
            ("d", project(&[], &["build"])),
        ],
        "build",
        None,
    );
    let sequenced = sequence(&mut diamond).unwrap();
    assert!(!is_serial_task_graph(&diamond, &sequenced));
}

#[test]
fn is_serial_sees_through_pass_through_tasks() {
    let mut graph = build_graph(
        &[
            ("a", project(&["b"], &["build"])),
            ("b", project(&["c"], &[])),
            ("c", project(&[], &["build"])),
        ],
        "build",
        None,
    );
    let sequenced = sequence(&mut graph).unwrap();
    assert!(is_serial_task_graph(&graph, &sequenced));
}

#[test]
fn json_document_emits_sorted_nodes_and_edges_with_a_missing_script_flag() {
    let graph =
        build_graph(&[("b", project(&["a"], &["build"])), ("a", project(&[], &[]))], "build", None);

    let document = task_graph_to_json(&graph, Path::new(WORKSPACE_DIR));
    let json = serde_json::to_value(&document).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "tasks": [
                {
                    "project": "a",
                    "script": "build",
                    "missingScript": true,
                    "dependsOn": [],
                },
                {
                    "project": "b",
                    "script": "build",
                    "missingScript": false,
                    "dependsOn": [{ "project": "a", "script": "build" }],
                },
            ],
        }),
    );
}

#[test]
fn dry_run_rendering_prints_one_stable_linearization() {
    let mut graph = build_graph(
        &[
            ("c", project(&["b"], &["build"])),
            ("b", project(&["a"], &[])),
            ("a", project(&[], &["build"])),
        ],
        "build",
        None,
    );
    let sequenced = sequence(&mut graph).unwrap();
    assert_eq!(
        render_task_graph_dry_run(&graph, &sequenced, Path::new(WORKSPACE_DIR)),
        "a#build\nb#build (skipped: no such script)\nc#build",
    );
}

#[test]
fn scheduler_runs_tasks_in_dependency_order() {
    let graph = build_graph(
        &[
            ("a", project(&["b"], &["build"])),
            ("b", project(&["c"], &[])),
            ("c", project(&[], &["build"])),
        ],
        "build",
        None,
    );
    let order: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let skipped: Mutex<Vec<String>> = Mutex::new(Vec::new());
    schedule_tasks(
        &graph,
        &ScheduleTasksOptions {
            concurrency: 4,
            bail: true,
            run_task: &|node| {
                order.lock().unwrap().push(node.project.to_string_lossy().into_owned());
                TaskCompletion::Passed
            },
            on_task_skipped: &|node| {
                skipped.lock().unwrap().push(node.project.to_string_lossy().into_owned());
            },
        },
    );
    assert_eq!(
        order.into_inner().unwrap(),
        vec![dir("c").to_string_lossy().into_owned(), dir("a").to_string_lossy().into_owned()],
    );
    assert_eq!(skipped.into_inner().unwrap(), vec![dir("b").to_string_lossy().into_owned()]);
}

#[test]
fn graph_scheduler_does_not_wait_for_an_unrelated_slow_branch() {
    let graph =
        IndexMap::from([("slow", Vec::new()), ("fast", Vec::new()), ("dependent", vec!["fast"])]);
    let release_slow = (Mutex::new(false), Condvar::new());
    let ran = Mutex::new(Vec::new());
    let on_node_skipped: fn(&&str) = |_| {};
    schedule_graph(
        &graph,
        &ScheduleGraphOptions {
            concurrency: 2,
            bail: true,
            continue_on_failure: false,
            run_node: &|node| {
                ran.lock().unwrap().push(node);
                if node == "slow" {
                    let (released, progress) = &release_slow;
                    let mut released = released.lock().unwrap();
                    while !*released {
                        released = progress.wait(released).unwrap();
                    }
                } else if node == "dependent" {
                    let (released, progress) = &release_slow;
                    *released.lock().unwrap() = true;
                    progress.notify_one();
                }
                TaskCompletion::Passed
            },
            on_node_skipped: &on_node_skipped,
        },
    )
    .unwrap();
    assert_eq!(ran.into_inner().unwrap(), vec!["slow", "fast", "dependent"]);
}

#[tokio::test]
async fn async_graph_scheduler_does_not_wait_for_an_unrelated_slow_branch() {
    let graph =
        IndexMap::from([("slow", Vec::new()), ("fast", Vec::new()), ("dependent", vec!["fast"])]);
    let release_slow = tokio::sync::Notify::new();
    let ran = Mutex::new(Vec::new());
    let run_node = |node| {
        let ran = &ran;
        let release_slow = &release_slow;
        async move {
            ran.lock().unwrap().push(node);
            if node == "slow" {
                release_slow.notified().await;
            } else if node == "dependent" {
                release_slow.notify_one();
            }
            TaskCompletion::Passed
        }
    };
    let on_node_skipped: fn(&&str) = |_| {};
    schedule_graph_async(
        &graph,
        &ScheduleGraphAsyncOptions::new(2, true, &run_node, &on_node_skipped),
    )
    .await;
    assert_eq!(ran.into_inner().unwrap(), vec!["slow", "fast", "dependent"]);
}

#[test]
fn scheduler_without_bail_skips_transitive_dependents_of_a_failure() {
    let graph = build_graph(
        &[
            ("a", project(&["b"], &["build"])),
            ("b", project(&[], &["build"])),
            ("unrelated", project(&[], &["build"])),
        ],
        "build",
        None,
    );
    let ran: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let skipped: Mutex<Vec<String>> = Mutex::new(Vec::new());
    schedule_tasks(
        &graph,
        &ScheduleTasksOptions {
            concurrency: 1,
            bail: false,
            run_task: &|node| {
                ran.lock().unwrap().push(node.project.to_string_lossy().into_owned());
                if node.project == dir("b") {
                    TaskCompletion::Failed
                } else {
                    TaskCompletion::Passed
                }
            },
            on_task_skipped: &|node| {
                skipped.lock().unwrap().push(node.project.to_string_lossy().into_owned());
            },
        },
    );
    let ran = ran.into_inner().unwrap();
    dbg!(&ran);
    assert!(ran.contains(&dir("unrelated").to_string_lossy().into_owned()));
    assert!(!ran.contains(&dir("a").to_string_lossy().into_owned()));
    assert_eq!(skipped.into_inner().unwrap(), vec![dir("a").to_string_lossy().into_owned()]);
}

#[test]
fn scheduler_with_bail_dispatches_nothing_after_a_failure() {
    let graph = build_graph(
        &[
            ("a", project(&[], &["build"])),
            ("b", project(&["a"], &["build"])),
            ("c", project(&["b"], &["build"])),
        ],
        "build",
        None,
    );
    let ran: Mutex<Vec<String>> = Mutex::new(Vec::new());
    schedule_tasks(
        &graph,
        &ScheduleTasksOptions {
            concurrency: 1,
            bail: true,
            run_task: &|node| {
                ran.lock().unwrap().push(node.project.to_string_lossy().into_owned());
                if node.project == dir("a") {
                    TaskCompletion::Failed
                } else {
                    TaskCompletion::Passed
                }
            },
            on_task_skipped: &|_| {
                unreachable!("bail leaves undispatched tasks queued, not skipped")
            },
        },
    );
    assert_eq!(ran.into_inner().unwrap(), vec![dir("a").to_string_lossy().into_owned()]);
}

#[test]
fn ignored_cycles_are_downgraded_and_backward_edges_are_dropped() {
    let mut graph = build_graph(
        &[
            ("a", project(&["b"], &["build"])),
            ("b", project(&["a"], &["build"])),
            ("c", project(&["a"], &["build"])),
        ],
        "build",
        None,
    );
    let groups = sequence_tasks(
        &mut graph,
        &SequenceTasksOptions {
            workspace_dir: Path::new(WORKSPACE_DIR),
            ignore_cycles: true,
            emit: drop_event,
        },
    )
    .unwrap();
    dbg!(&groups);
    // The backward cycle edge is dropped while the forward edge preserves
    // a deterministic order; the task outside the cycle still waits.
    assert!(graph[&key("a", "build")].dependencies.is_empty());
    assert_eq!(graph[&key("b", "build")].dependencies, vec![key("a", "build")]);
    assert_eq!(graph[&key("c", "build")].dependencies, vec![key("a", "build")]);
}

#[test]
fn scheduler_propagates_a_run_task_panic_instead_of_hanging() {
    let graph = build_graph(
        &[
            ("a", project(&[], &["build"])),
            ("b", project(&[], &["build"])),
            ("c", project(&[], &["build"])),
        ],
        "build",
        None,
    );
    let result = std::panic::catch_unwind(|| {
        schedule_tasks(
            &graph,
            &ScheduleTasksOptions {
                concurrency: 3,
                bail: true,
                run_task: &|node| {
                    assert!(node.project != dir("a"), "boom");
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    TaskCompletion::Passed
                },
                on_task_skipped: &|_| {},
            },
        );
    });
    assert!(result.is_err(), "the worker panic must reach the caller");
}
