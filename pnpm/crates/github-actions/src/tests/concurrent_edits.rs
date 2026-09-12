use super::{FakeGitRunner, GitCommandRunner, GitRunError, SilentReporter, update_with_runner};
use std::{fs, future::Future, path::PathBuf, pin::Pin};

struct EditingGitRunner {
    workflow: PathBuf,
    source: String,
}

impl GitCommandRunner for EditingGitRunner {
    fn ls_remote<'a>(
        &'a self,
        repo: &'a str,
        ref_: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, GitRunError>> + Send + 'a>> {
        Box::pin(async move {
            fs::write(&self.workflow, &self.source).expect("concurrent edit");
            FakeGitRunner.ls_remote(repo, ref_).await
        })
    }
}

#[tokio::test]
async fn preserves_concurrent_workflow_edits_when_action_ranges_are_stale() {
    let original = "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@v4\n";
    let start = original.find("actions/checkout").expect("action");
    for changed in [
        original.replace("checkout@v4", "checkout@v5"),
        "name: truncated\n".to_string(),
        format!("{}é{}", " ".repeat(start - 1), " ".repeat(original.len())),
    ] {
        let root = tempfile::tempdir().expect("temp directory");
        let directory = root.path().join(".github/workflows");
        fs::create_dir_all(&directory).expect("workflow directory");
        let workflow = directory.join("ci.yml");
        fs::write(&workflow, original).expect("workflow");
        let runner = EditingGitRunner { workflow: workflow.clone(), source: changed.clone() };
        let result = update_with_runner::<SilentReporter, _>(
            root.path(),
            false,
            None,
            "https://github.com",
            &runner,
        )
        .await;
        let error = result.err().expect("stale edits must fail");
        assert_eq!(
            error.code().expect("error code").to_string(),
            "ERR_PNPM_GITHUB_ACTIONS_WORKFLOW_CHANGED",
        );
        assert_eq!(fs::read_to_string(workflow).expect("workflow"), changed);
    }
}

#[tokio::test]
async fn keeps_unrelated_edits_when_action_ranges_still_match() {
    let original = "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@v4\n";
    let root = tempfile::tempdir().expect("temp directory");
    let directory = root.path().join(".github/workflows");
    fs::create_dir_all(&directory).expect("workflow directory");
    let workflow = directory.join("ci.yml");
    fs::write(&workflow, original).expect("workflow");
    let runner = EditingGitRunner {
        workflow: workflow.clone(),
        source: format!("{original}# concurrent note\n"),
    };
    update_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &runner,
    )
    .await
    .expect("update actions");
    let updated = fs::read_to_string(workflow).expect("workflow");
    assert!(updated.contains(super::SHA_V4_2_0));
    assert!(updated.ends_with("# concurrent note\n"));
}
