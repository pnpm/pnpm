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
