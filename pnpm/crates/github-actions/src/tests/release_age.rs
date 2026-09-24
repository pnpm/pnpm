use super::{FakeGitRunner, SHA_V4_1_0, SHA_V4_2_0, SilentReporter};
use crate::{
    ReleaseAge, ReleaseAgeCheck, find_outdated_with_runner,
    release_age::{TagDateReader, TagDates},
    update_with_runner,
};
use chrono::{DateTime, Utc};
use node_semver::Version;
use pnpm_config::version_policy::create_package_version_policy;
use std::{fs, future::Future, path::Path, pin::Pin, sync::Mutex};

const CUTOFF: i64 = 1_700_000_000;

/// v4.2.0 was tagged before the cutoff, v5.0.0 after it.
struct FakeTagDates {
    requested: Mutex<Vec<String>>,
}

impl FakeTagDates {
    fn new() -> Self {
        Self { requested: Mutex::new(Vec::new()) }
    }
}

impl TagDateReader for FakeTagDates {
    fn read_tag_dates<'a>(
        &'a self,
        _repo_url: &'a str,
        tags: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<TagDates, String>> + Send + 'a>> {
        self.requested
            .lock()
            .expect("lock")
            .extend(tags.iter().cloned());
        Box::pin(async move {
            Ok(tags
                .iter()
                .filter_map(|tag| match tag.as_str() {
                    "v4.2.0" => Some((tag.clone(), CUTOFF - 60)),
                    "v5.0.0" => Some((tag.clone(), CUTOFF + 60)),
                    _ => None,
                })
                .collect())
        })
    }
}

struct FailingTagDates;

impl TagDateReader for FailingTagDates {
    fn read_tag_dates<'a>(
        &'a self,
        _repo_url: &'a str,
        _tags: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<TagDates, String>> + Send + 'a>> {
        Box::pin(async { Err("fetch failed".to_string()) })
    }
}

fn policy(exclude: &[&str]) -> ReleaseAge {
    ReleaseAge {
        published_by: DateTime::<Utc>::from_timestamp(CUTOFF, 0).expect("cutoff"),
        exclude: (!exclude.is_empty()).then(|| {
            create_package_version_policy(exclude).expect("exclude policy")
        }),
    }
}

fn workflow(root: &Path) -> std::path::PathBuf {
    let workflows = root.join(".github/workflows");
    fs::create_dir_all(&workflows).expect("workflow directory");
    let workflow = workflows.join("ci.yml");
    fs::write(
        &workflow,
        format!(
            "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@{SHA_V4_1_0} # v4.1.0\n",
        ),
    )
    .expect("workflow");
    workflow
}

#[tokio::test]
async fn offers_only_versions_older_than_the_minimum_release_age() {
    let root = tempfile::tempdir().expect("temp directory");
    workflow(root.path());
    let policy = policy(&[]);
    let dates = FakeTagDates::new();

    let outdated = find_outdated_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        Some(ReleaseAgeCheck { policy: &policy, dates: &dates }),
    )
    .await
    .expect("outdated");

    assert_eq!(outdated.len(), 1);
    assert_eq!(outdated[0].latest, Version::parse("4.2.0").unwrap());
    assert_eq!(*dates.requested.lock().expect("lock"), ["v4.2.0", "v5.0.0"]);
}

#[tokio::test]
async fn updates_to_the_newest_version_old_enough() {
    let root = tempfile::tempdir().expect("temp directory");
    let workflow = workflow(root.path());
    let policy = policy(&[]);

    update_with_runner::<SilentReporter, _>(
        root.path(),
        true,
        None,
        "https://github.com",
        &FakeGitRunner,
        Some(ReleaseAgeCheck { policy: &policy, dates: &FakeTagDates::new() }),
    )
    .await
    .expect("update");

    assert!(
        fs::read_to_string(&workflow)
            .expect("updated workflow")
            .contains(&format!("uses: actions/checkout@{SHA_V4_2_0} # v4.2.0")),
    );
}

#[tokio::test]
async fn excluded_actions_skip_the_release_age_check() {
    for exclude in ["actions/checkout", "actions/*", "actions/checkout@5.0.0"] {
        let root = tempfile::tempdir().expect("temp directory");
        workflow(root.path());
        let policy = policy(&[exclude]);

        let outdated = find_outdated_with_runner::<SilentReporter, _>(
            root.path(),
            false,
            None,
            "https://github.com",
            &FakeGitRunner,
            Some(ReleaseAgeCheck { policy: &policy, dates: &FakeTagDates::new() }),
        )
        .await
        .expect("outdated");

        assert_eq!(outdated[0].latest, Version::parse("5.0.0").unwrap(), "{exclude}");
    }
}

#[tokio::test]
async fn skips_actions_whose_release_dates_cannot_be_read() {
    let root = tempfile::tempdir().expect("temp directory");
    workflow(root.path());
    let policy = policy(&[]);

    let outdated = find_outdated_with_runner::<SilentReporter, _>(
        root.path(),
        false,
        None,
        "https://github.com",
        &FakeGitRunner,
        Some(ReleaseAgeCheck { policy: &policy, dates: &FailingTagDates }),
    )
    .await
    .expect("outdated");

    assert!(outdated.is_empty());
}
