use super::{GitTagDates, TagDateReader};
use std::{path::Path, process::Command};

fn git(dir: &Path, args: &[&str], date: &str) {
    // The user's own git config may sign tags or commits.
    let status = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", dir.join(".no-global-config"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=pnpm", "-c", "user.email=pnpm@example.com"])
        .args(args)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

#[tokio::test]
async fn reads_the_tagger_date_of_annotated_tags_and_the_commit_date_of_lightweight_ones() {
    let repo = tempfile::tempdir().expect("temp directory");
    git(repo.path(), &["init", "--quiet"], "1600000000 +0000");
    git(repo.path(), &["commit", "--quiet", "--allow-empty", "-m", "one"], "1600000000 +0000");
    git(repo.path(), &["tag", "v1.0.0"], "1600000000 +0000");
    git(repo.path(), &["tag", "-a", "-m", "v1.1.0", "v1.1.0"], "1650000000 +0000");
    git(repo.path(), &["commit", "--quiet", "--allow-empty", "-m", "two"], "1700000000 +0000");
    git(repo.path(), &["tag", "v2.0.0"], "1700000000 +0000");

    let url = url::Url::from_directory_path(repo.path()).expect("file URL").to_string();
    let dates = GitTagDates
        .read_tag_dates(&url, &["v1.1.0".to_string(), "v2.0.0".to_string()])
        .await
        .expect("tag dates");

    assert_eq!(dates.len(), 2);
    assert_eq!(dates["v1.1.0"], 1_650_000_000);
    assert_eq!(dates["v2.0.0"], 1_700_000_000);
}
