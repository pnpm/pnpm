use super::{FakeGitRunner, SHA_V4_1_0, SHA_V4_2_0, SilentReporter, update_with_runner};
use crate::edits::{WorkflowEdit, apply_replacements};
use pnpm_resolving_git_resolver::{GitCommandRunner, GitRunError};
use std::{fs, future::Future, path::Path, pin::Pin};

const SHA_V4_1_1000: &str = "4444444444444444444444444444444444444444";

struct ShrinkingTagGitRunner;

impl GitCommandRunner for ShrinkingTagGitRunner {
    fn ls_remote<'a>(
        &'a self,
        _repo: &'a str,
        _ref_: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, GitRunError>> + Send + 'a>> {
        Box::pin(async {
            Ok(format!("{SHA_V4_1_1000}\trefs/tags/v4.1.1000\n{SHA_V4_2_0}\trefs/tags/v4.2.0\n"))
        })
    }
}

#[tokio::test]
async fn updates_anchored_steps_shared_by_aliases_with_multibyte_utf8_comments() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");
    let source = format!(
        concat!(
            "on: push\n",
            "jobs:\n",
            "  first:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps: &shared\n",
            "      - uses: 'actions/checkout@{0}' # v4.1.1000\n",
            "# café: éééé\n",
            "  second:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps: *shared\n",
            "  third:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps: *shared\n",
        ),
        SHA_V4_1_1000,
    );
    fs::write(&workflow, &source).expect("write workflow");

    let updated_actions = update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &ShrinkingTagGitRunner,
        None,
    )
    .await
    .expect("update anchored workflow");
    assert_eq!(updated_actions.len(), 1);

    let updated = fs::read_to_string(&workflow).expect("read updated workflow");
    assert_eq!(
        updated,
        format!(
            concat!(
                "on: push\n",
                "jobs:\n",
                "  first:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps: &shared\n",
                "      - uses: 'actions/checkout@{0}' # v4.2.0\n",
                "# café: éééé\n",
                "  second:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps: *shared\n",
                "  third:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps: *shared\n",
            ),
            SHA_V4_2_0,
        ),
    );
    let parsed: yaml_serde::Value =
        yaml_serde::from_str(&updated).expect("updated workflow must remain valid YAML");
    let expected_uses = format!("actions/checkout@{SHA_V4_2_0}");
    for job_name in ["first", "second", "third"] {
        assert_eq!(
            parsed["jobs"][job_name]["steps"][0]["uses"].as_str(),
            Some(expected_uses.as_str()),
        );
    }
}

#[tokio::test]
async fn updates_anchored_step_mappings_scalar_anchors_and_merge_keys_once() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");
    let source = format!(
        concat!(
            "x-checkout: &top_checkout\n",
            "  uses: actions/checkout@{0} # v4.1.0\n",
            "x-setup: &top_scalar actions/setup-python@{0} # v4.1.0\n",
            "jobs:\n",
            "  first:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - <<: *top_checkout\n",
            "        name: Checkout A\n",
            "      - &inline_step\n",
            "        uses: actions/cache@{0} # v4.1.0\n",
            "      - uses: &scalar_ref actions/setup-node@{0} # v4.1.0\n",
            "      - uses: *top_scalar\n",
            "  second:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - <<: [*top_checkout]\n",
            "        name: Checkout B\n",
            "      - *inline_step\n",
            "      - <<: *inline_step\n",
            "        name: Cache B\n",
            "      - uses: *scalar_ref\n",
            "      - uses: *top_scalar\n",
        ),
        SHA_V4_1_0,
    );
    fs::write(&workflow, &source).expect("write workflow");

    update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        None,
    )
    .await
    .expect("update anchored workflow");

    let updated = fs::read_to_string(&workflow).expect("read updated workflow");
    assert_eq!(
        updated,
        format!(
            concat!(
                "x-checkout: &top_checkout\n",
                "  uses: actions/checkout@{0} # v4.2.0\n",
                "x-setup: &top_scalar actions/setup-python@{0} # v4.2.0\n",
                "jobs:\n",
                "  first:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps:\n",
                "      - <<: *top_checkout\n",
                "        name: Checkout A\n",
                "      - &inline_step\n",
                "        uses: actions/cache@{0} # v4.2.0\n",
                "      - uses: &scalar_ref actions/setup-node@{0} # v4.2.0\n",
                "      - uses: *top_scalar\n",
                "  second:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps:\n",
                "      - <<: [*top_checkout]\n",
                "        name: Checkout B\n",
                "      - *inline_step\n",
                "      - <<: *inline_step\n",
                "        name: Cache B\n",
                "      - uses: *scalar_ref\n",
                "      - uses: *top_scalar\n",
            ),
            SHA_V4_2_0,
        ),
    );
    yaml_serde::from_str::<yaml_serde::Value>(&updated)
        .expect("updated workflow must remain valid YAML");
}

#[test]
fn does_not_rewalk_shared_yaml_merge_key_dags_exponentially() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let mut dag_lines = vec!["m0: &m0 { runs-on: ubuntu-latest }".to_string()];
    for level in 1..=8 {
        let refs =
            std::iter::repeat_n(format!("*m{}", level - 1), 20).collect::<Vec<_>>().join(", ");
        dag_lines.push(format!("m{level}: &m{level} {{ <<: [{refs}] }}"));
    }
    fs::write(
        workflows.join("ci.yml"),
        format!("{}\njobs:\n  build:\n    <<: *m8\n", dag_lines.join("\n")),
    )
    .expect("write workflow");

    let root_path = root.path().to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build current-thread runtime");
        let result = runtime.block_on(update_with_runner::<SilentReporter, _>(
            &root_path,
            false,
            None,
            "https://github.com",
            &FakeGitRunner,
            None,
        ));
        let _ = sender.send(result);
    });

    let updated = receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("shared merge DAG scan must not hang")
        .expect("scan shared merge DAG");
    assert!(updated.is_empty());
}

#[tokio::test]
async fn discovers_actions_on_shallower_merge_branch_after_deep_branch_hits_cap() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");
    let source = format!(
        concat!(
            "base: &base\n",
            "  uses: actions/checkout@{0} # v4.1.0\n",
            "shared: &shared\n",
            "  <<: *base\n",
            "d7: &d7 {{ <<: *shared }}\n",
            "d6: &d6 {{ <<: *d7 }}\n",
            "d5: &d5 {{ <<: *d6 }}\n",
            "d4: &d4 {{ <<: *d5 }}\n",
            "d3: &d3 {{ <<: *d4 }}\n",
            "d2: &d2 {{ <<: *d3 }}\n",
            "d1: &d1 {{ <<: *d2 }}\n",
            "jobs:\n",
            "  build:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - <<: [*d1, *shared]\n",
            "        name: Checkout\n",
        ),
        SHA_V4_1_0,
    );
    fs::write(&workflow, &source).expect("write workflow");

    let updated_actions = update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        None,
    )
    .await
    .expect("update shallow shared merge workflow");
    assert_eq!(updated_actions.len(), 1);
    let updated_source = fs::read_to_string(&workflow).expect("read updated workflow");
    assert!(updated_source.contains(&format!("uses: actions/checkout@{SHA_V4_2_0} # v4.2.0")));
}

#[tokio::test]
async fn updates_action_in_large_step_sequence() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");
    let mut lines = vec![
        "jobs:".to_string(),
        "  build:".to_string(),
        "    runs-on: ubuntu-latest".to_string(),
        "    steps:".to_string(),
    ];
    for step_idx in 0..200 {
        lines.push(format!("      - name: Run {step_idx}"));
        lines.push(format!("        run: echo {step_idx}"));
    }
    lines.push(format!("      - uses: actions/checkout@{SHA_V4_1_0} # v4.1.0"));
    fs::write(&workflow, format!("{}\n", lines.join("\n"))).expect("write workflow");

    let updated_actions = update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        None,
    )
    .await
    .expect("update large step workflow");
    assert_eq!(updated_actions.len(), 1);
    let updated_source = fs::read_to_string(&workflow).expect("read updated workflow");
    assert!(updated_source.contains(&format!("uses: actions/checkout@{SHA_V4_2_0} # v4.2.0")));
}

#[tokio::test]
async fn updates_actions_from_root_and_jobs_map_merges_with_direct_overrides() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");

    let root_merged = workflows.join("root-merged.yml");
    fs::write(
        &root_merged,
        format!(
            concat!(
                "template: &template\n",
                "  jobs:\n",
                "    lint:\n",
                "      runs-on: ubuntu-latest\n",
                "      steps:\n",
                "        - uses: actions/checkout@{0} # v4.1.0\n",
                "<<: *template\n",
            ),
            SHA_V4_1_0,
        ),
    )
    .expect("write root-merged workflow");

    let jobs_merged = workflows.join("jobs-merged.yml");
    fs::write(
        &jobs_merged,
        format!(
            concat!(
                "ignored_root: &ignored_root\n",
                "  jobs:\n",
                "    shadowed_root_job:\n",
                "      runs-on: ubuntu-latest\n",
                "      steps:\n",
                "        - uses: actions/checkout@{0} # v4.1.0\n",
                "shared_jobs: &shared_jobs\n",
                "  inherited_job:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps:\n",
                "      - uses: actions/checkout@{0} # v4.1.0\n",
                "  overridden_job:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps:\n",
                "      - uses: actions/checkout@{0} # v4.1.0\n",
                "<<: *ignored_root\n",
                "jobs:\n",
                "  <<: *shared_jobs\n",
                "  overridden_job:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps:\n",
                "      - uses: actions/setup-node@{0} # v4.1.0\n",
            ),
            SHA_V4_1_0,
        ),
    )
    .expect("write jobs-merged workflow");

    let updated_actions = update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        None,
    )
    .await
    .expect("update merged workflows");
    let mut updated_names = updated_actions
        .into_iter()
        .map(|action| action.name)
        .collect::<Vec<_>>();
    updated_names.sort();
    assert_eq!(
        updated_names,
        vec!["actions/checkout".to_string(), "actions/setup-node".to_string()],
    );

    assert_eq!(
        fs::read_to_string(&root_merged).expect("read root-merged workflow"),
        format!(
            concat!(
                "template: &template\n",
                "  jobs:\n",
                "    lint:\n",
                "      runs-on: ubuntu-latest\n",
                "      steps:\n",
                "        - uses: actions/checkout@{0} # v4.2.0\n",
                "<<: *template\n",
            ),
            SHA_V4_2_0,
        ),
    );
    assert_eq!(
        fs::read_to_string(&jobs_merged).expect("read jobs-merged workflow"),
        format!(
            concat!(
                "ignored_root: &ignored_root\n",
                "  jobs:\n",
                "    shadowed_root_job:\n",
                "      runs-on: ubuntu-latest\n",
                "      steps:\n",
                "        - uses: actions/checkout@{0} # v4.1.0\n",
                "shared_jobs: &shared_jobs\n",
                "  inherited_job:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps:\n",
                "      - uses: actions/checkout@{1} # v4.2.0\n",
                "  overridden_job:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps:\n",
                "      - uses: actions/checkout@{0} # v4.1.0\n",
                "<<: *ignored_root\n",
                "jobs:\n",
                "  <<: *shared_jobs\n",
                "  overridden_job:\n",
                "    runs-on: ubuntu-latest\n",
                "    steps:\n",
                "      - uses: actions/setup-node@{1} # v4.2.0\n",
            ),
            SHA_V4_1_0, SHA_V4_2_0,
        ),
    );
}

#[test]
fn apply_replacements_deduplicates_identical_edits_and_rejects_overlapping_ones() {
    let text = "uses: 'actions/checkout@old' # v4.1.1000\n# café: éééé\n";
    let duplicate_edits = vec![
        WorkflowEdit {
            range: 6..40,
            expected: "'actions/checkout@old' # v4.1.1000".to_string(),
            replacement: "'actions/checkout@new' # v4.2.0".to_string(),
        },
        WorkflowEdit {
            range: 6..40,
            expected: "'actions/checkout@old' # v4.1.1000".to_string(),
            replacement: "'actions/checkout@new' # v4.2.0".to_string(),
        },
    ];
    let applied = apply_replacements(Path::new("ci.yml"), text.to_string(), duplicate_edits)
        .expect("identical duplicate edits should apply once");
    assert_eq!(applied, "uses: 'actions/checkout@new' # v4.2.0\n# café: éééé\n");

    let conflicting_same_range = vec![
        WorkflowEdit {
            range: 6..40,
            expected: "'actions/checkout@old' # v4.1.1000".to_string(),
            replacement: "'actions/checkout@one' # v4.2.0".to_string(),
        },
        WorkflowEdit {
            range: 6..40,
            expected: "'actions/checkout@old' # v4.1.1000".to_string(),
            replacement: "'actions/checkout@two' # v4.2.0".to_string(),
        },
    ];
    let conflict_err =
        apply_replacements(Path::new("ci.yml"), text.to_string(), conflicting_same_range)
            .expect_err("conflicting edits on same span must fail");
    assert_eq!(
        conflict_err
            .code()
            .expect("error code")
            .to_string(),
        "ERR_PNPM_GITHUB_ACTIONS_OVERLAPPING_EDITS",
    );

    let overlapping = vec![
        WorkflowEdit {
            range: 6..24,
            expected: "'actions/checkout@".to_string(),
            replacement: "'actions/checkout@a".to_string(),
        },
        WorkflowEdit {
            range: 15..40,
            expected: "checkout@old' # v4.1.1000".to_string(),
            replacement: "checkout@b' # v4.2.0".to_string(),
        },
    ];
    let overlap_err = apply_replacements(Path::new("ci.yml"), text.to_string(), overlapping)
        .expect_err("partially overlapping edits must fail");
    assert_eq!(
        overlap_err
            .code()
            .expect("error code")
            .to_string(),
        "ERR_PNPM_GITHUB_ACTIONS_OVERLAPPING_EDITS",
    );
}

#[tokio::test]
async fn ignores_quoted_and_str_tagged_merge_lookalikes_while_honoring_explicit_merge_tags() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");
    let source = format!(
        concat!(
            "inactive_root: &inactive_root\n",
            "  jobs:\n",
            "    unused:\n",
            "      runs-on: ubuntu-latest\n",
            "      steps:\n",
            "        - uses: actions/setup-python@{0} # v4.1.0\n",
            "inactive_step: &inactive_step\n",
            "  uses: actions/cache@{0} # v4.1.0\n",
            "active_step: &active_step\n",
            "  uses: actions/checkout@{0} # v4.1.0\n",
            "plain_merge_step: &plain_merge_step\n",
            "  uses: actions/setup-node@{0} # v4.1.0\n",
            "active_jobs: &active_jobs\n",
            "  build:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - \"<<\": *inactive_step\n",
            "        !!merge <<: *active_step\n",
            "      - \"<<\": *inactive_step\n",
            "        <<: *plain_merge_step\n",
            "      - '<<': *inactive_step\n",
            "        run: echo single-quoted\n",
            "      - !!str <<: *inactive_step\n",
            "        run: echo str-tagged\n",
            "\"<<\": *inactive_root\n",
            "jobs:\n",
            "  '<<': *inactive_root\n",
            "  !!merge <<: *active_jobs\n",
        ),
        SHA_V4_1_0,
    );
    fs::write(&workflow, &source).expect("write workflow");

    let updated_actions = update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        None,
    )
    .await
    .expect("update workflow with literal << keys and !!merge");
    let mut names = updated_actions
        .into_iter()
        .map(|action| action.name)
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["actions/checkout".to_string(), "actions/setup-node".to_string()]);

    let updated = fs::read_to_string(&workflow).expect("read updated workflow");
    assert!(
        updated.contains(&format!("uses: actions/setup-python@{SHA_V4_1_0} # v4.1.0")),
        "{updated}",
    );
    assert!(updated.contains(&format!("uses: actions/cache@{SHA_V4_1_0} # v4.1.0")), "{updated}");
    assert!(
        updated.contains(&format!("uses: actions/checkout@{SHA_V4_2_0} # v4.2.0")),
        "{updated}",
    );
    assert!(
        updated.contains(&format!("uses: actions/setup-node@{SHA_V4_2_0} # v4.2.0")),
        "{updated}",
    );
}

#[tokio::test]
async fn updates_escaped_job_keys_and_next_line_anchored_uses_scalars() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");
    let source = format!(
        concat!(
            "\"pin#id\": &pin # explanation\n",
            "  actions/checkout@{0} # v4.1.0\n",
            "shared_jobs: &shared_jobs\n",
            "  \"bu\\u0069ld\":\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - uses: *pin\n",
            "  \"li\\u006et\":\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - uses: actions/cache@{0} # v4.1.0\n",
            "jobs:\n",
            "  <<: *shared_jobs\n",
            "  lint:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - uses: actions/setup-node@{0} # v4.1.0\n",
        ),
        SHA_V4_1_0,
    );
    fs::write(&workflow, &source).expect("write workflow");

    let updated_actions = update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        None,
    )
    .await
    .expect("update escaped job keys and multiline anchor scalar");
    let mut names = updated_actions
        .into_iter()
        .map(|action| action.name)
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["actions/checkout".to_string(), "actions/setup-node".to_string()]);

    let updated = fs::read_to_string(&workflow).expect("read updated workflow");
    assert!(
        updated.contains(&format!(
            "\"pin#id\": &pin # explanation\n  actions/checkout@{SHA_V4_2_0} # v4.2.0\n",
        )),
        "{updated}",
    );
    assert!(updated.contains(&format!("uses: actions/cache@{SHA_V4_1_0} # v4.1.0")), "{updated}");
    assert!(
        updated.contains(&format!("uses: actions/setup-node@{SHA_V4_2_0} # v4.2.0")),
        "{updated}",
    );
}

#[tokio::test]
async fn stops_fallback_after_merge_depth_cutoff_while_keeping_direct_overrides() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");

    let mut lines = vec![
        format!("s0: &s0 {{ uses: actions/checkout@{SHA_V4_1_0} }}"),
        format!("shadowed_step: &shadowed_step {{ uses: actions/setup-node@{SHA_V4_1_0} }}"),
        "j0: &j0".to_string(),
        "  ci:".to_string(),
        "    runs-on: ubuntu-latest".to_string(),
        "    steps:".to_string(),
        format!("      - uses: actions/checkout@{SHA_V4_1_0}"),
        "shadowed_jobs: &shadowed_jobs".to_string(),
        "  ci:".to_string(),
        "    runs-on: ubuntu-latest".to_string(),
        "    steps:".to_string(),
        format!("      - uses: actions/setup-python@{SHA_V4_1_0}"),
    ];
    for depth in 1..=65 {
        lines.push(format!("s{depth}: &s{depth} {{ <<: *s{} }}", depth - 1));
        lines.push(format!("j{depth}: &j{depth} {{ <<: *j{} }}", depth - 1));
    }
    lines.extend([
        "jobs:".to_string(),
        "  <<: [*j65, *shadowed_jobs]".to_string(),
        "  direct:".to_string(),
        "    runs-on: ubuntu-latest".to_string(),
        "    steps:".to_string(),
        "      - <<: [*s65, *shadowed_step]".to_string(),
        "        name: Deep cutoff step".to_string(),
        format!("      - uses: actions/cache@{SHA_V4_1_0}"),
        "        <<: [*s65, *shadowed_step]".to_string(),
    ]);
    fs::write(&workflow, format!("{}\n", lines.join("\n"))).expect("write workflow");

    let updated_actions = update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        None,
    )
    .await
    .expect("update workflow hitting merge depth cutoff");
    assert_eq!(
        updated_actions
            .into_iter()
            .map(|action| action.name)
            .collect::<Vec<_>>(),
        vec!["actions/cache".to_string()],
    );

    let updated = fs::read_to_string(&workflow).expect("read updated workflow");
    assert!(updated.contains(&format!("actions/setup-node@{SHA_V4_1_0}")), "{updated}");
    assert!(updated.contains(&format!("actions/setup-python@{SHA_V4_1_0}")), "{updated}");
    assert!(updated.contains(&format!("actions/cache@{SHA_V4_2_0} # v4.2.0")), "{updated}");
}

#[test]
fn bounds_cyclic_shared_yaml_merge_graphs_in_lookup_and_collection() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");

    let mut step_layers = vec![
        "linear_cycle: &linear_cycle { <<: *linear_cycle }".to_string(),
        "step_cycle: &step_cycle".to_string(),
        "  m1: &sm1 { <<: *step_cycle }".to_string(),
    ];
    for level in 2..=7 {
        let step_refs =
            std::iter::repeat_n(format!("*sm{}", level - 1), 10).collect::<Vec<_>>().join(", ");
        step_layers.push(format!("  m{level}: &sm{level} {{ <<: [{step_refs}] }}"));
    }
    step_layers.push("  <<: *sm7".to_string());

    let mut active_jobs_cycle = vec!["  self_m1: &jm1 { <<: *jobs_self }".to_string()];
    for level in 2..=7 {
        let job_refs =
            std::iter::repeat_n(format!("*jm{}", level - 1), 10).collect::<Vec<_>>().join(", ");
        active_jobs_cycle.push(format!("  self_m{level}: &jm{level} {{ <<: [{job_refs}] }}"));
    }

    let source = format!(
        concat!(
            "{0}\n",
            "inherited_jobs: &inherited_jobs\n",
            "  lint:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - uses: actions/setup-node@{2} # v4.1.0\n",
            "fallback_step: &fallback_step\n",
            "  uses: actions/checkout@{2} # v4.1.0\n",
            "jobs: &jobs_self\n",
            "{1}\n",
            "  <<: [*linear_cycle, *jm7, *inherited_jobs]\n",
            "  build:\n",
            "    runs-on: ubuntu-latest\n",
            "    steps:\n",
            "      - <<: [*linear_cycle, *step_cycle]\n",
            "        run: echo empty-cycle\n",
            "      - <<: [*step_cycle, *fallback_step]\n",
        ),
        step_layers.join("\n"),
        active_jobs_cycle.join("\n"),
        SHA_V4_1_0,
    );
    fs::write(&workflow, &source).expect("write cyclic workflow");

    let root_path = root.path().to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build current-thread runtime");
        let result = runtime.block_on(update_with_runner::<SilentReporter, _>(
            &root_path,
            false,
            None,
            "https://github.com",
            &FakeGitRunner,
            None,
        ));
        let _ = sender.send(result);
    });

    let updated_actions = receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("cyclic shared merge graph scan must not hang")
        .expect("update cyclic shared merge workflow");
    let mut names = updated_actions
        .into_iter()
        .map(|action| action.name)
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["actions/checkout".to_string(), "actions/setup-node".to_string()]);
    let updated = fs::read_to_string(&workflow).expect("read updated workflow");
    assert!(
        updated.contains(&format!("uses: actions/checkout@{SHA_V4_2_0} # v4.2.0")),
        "{updated}",
    );
    assert!(
        updated.contains(&format!("uses: actions/setup-node@{SHA_V4_2_0} # v4.2.0")),
        "{updated}",
    );
}

#[tokio::test]
async fn updates_escaped_double_quoted_uses_scalars_and_skips_block_scalars() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflows = root.path().join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");
    let shared_jobs = (0..50)
        .map(|index| {
            format!("  job_{index}:\n    runs-on: ubuntu-latest\n    steps: *shared_steps")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let source = format!(
        concat!(
            "escaped_pin: &escaped_pin \"actions\\/checkout\\u0040{0}\" # v4.1.0 keep\n",
            "block_folded: &block_folded >-\n",
            "  actions/cache@{0}\n",
            "block_literal: &block_literal |-\n",
            "  actions/setup-python@{0}\n",
            "shared_steps: &shared_steps\n",
            "  - uses: *escaped_pin\n",
            "  - uses: \"actions/setup-node\\u0040v4.1.0\"\n",
            "  - uses: *block_folded\n",
            "  - uses: *block_literal\n",
            "  - uses: >-\n",
            "      actions/upload-artifact@{0}\n",
            "jobs:\n",
            "{1}\n",
        ),
        SHA_V4_1_0, shared_jobs,
    );
    fs::write(&workflow, &source).expect("write workflow");

    let updated_actions = update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        None,
    )
    .await
    .expect("update escaped and block scalar workflow");
    let mut names = updated_actions
        .into_iter()
        .map(|action| action.name)
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["actions/checkout".to_string(), "actions/setup-node".to_string()]);

    let updated = fs::read_to_string(&workflow).expect("read updated workflow");
    assert!(
        updated.contains(&format!(
            "escaped_pin: &escaped_pin \"actions/checkout@{SHA_V4_2_0}\" # v4.2.0 keep\n",
        )),
        "{updated}",
    );
    assert!(
        updated.contains(&format!("  - uses: \"actions/setup-node@{SHA_V4_2_0}\" # v4.2.0\n")),
        "{updated}",
    );
    let parsed: yaml_serde::Value =
        yaml_serde::from_str(&updated).expect("updated YAML must remain valid");
    let steps = parsed["jobs"]["job_0"]["steps"].as_sequence().expect("steps sequence");
    assert_eq!(steps[0]["uses"].as_str(), Some(format!("actions/checkout@{SHA_V4_2_0}").as_str()));
    assert_eq!(
        steps[1]["uses"].as_str(),
        Some(format!("actions/setup-node@{SHA_V4_2_0}").as_str()),
    );
    assert_eq!(steps[2]["uses"].as_str(), Some(format!("actions/cache@{SHA_V4_1_0}").as_str()));
    assert_eq!(
        steps[3]["uses"].as_str(),
        Some(format!("actions/setup-python@{SHA_V4_1_0}").as_str()),
    );
    assert_eq!(
        steps[4]["uses"].as_str(),
        Some(format!("actions/upload-artifact@{SHA_V4_1_0}").as_str()),
    );
}
