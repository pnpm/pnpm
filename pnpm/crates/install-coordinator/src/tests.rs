use super::{InstallPlan, InstallTask, PreparedInstall};
use miette::{IntoDiagnostic, Result, bail};
use std::{
    fs,
    future::poll_fn,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    task::Poll,
    time::Duration,
};

struct Projection {
    visible: PathBuf,
    resources: tempfile::TempDir,
    previous: Option<String>,
    fail_publish: bool,
    fail_rollback: bool,
    rollback_dependency: Option<(PathBuf, PathBuf)>,
}

impl Projection {
    fn new(root: &std::path::Path, name: &str) -> Self {
        Self {
            visible: root.join(name),
            resources: tempfile::tempdir_in(root).unwrap(),
            previous: None,
            fail_publish: false,
            fail_rollback: false,
            rollback_dependency: None,
        }
    }
}

impl PreparedInstall for Projection {
    fn publish(&mut self) -> Result<()> {
        self.previous = match fs::read_to_string(&self.visible) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).into_diagnostic(),
        };
        fs::write(&self.visible, self.resources.path().to_str().unwrap()).into_diagnostic()?;
        if self.fail_publish {
            bail!("publication failed");
        }
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        if let Some((path, expected)) = &self.rollback_dependency {
            assert_eq!(fs::read_to_string(path).unwrap(), expected.to_str().unwrap());
        }
        if self.fail_rollback {
            bail!("rollback failed");
        }
        match &self.previous {
            Some(previous) => fs::write(&self.visible, previous),
            None => fs::remove_file(&self.visible),
        }
        .into_diagnostic()
    }

    fn retain(self: Box<Self>) {
        let _ = self.resources.keep();
    }
}

#[tokio::test]
async fn polls_participants_concurrently() {
    let root = tempfile::tempdir().unwrap();
    let started = Arc::new(AtomicU8::new(0));
    let installer = |own| {
        let started = Arc::clone(&started);
        poll_fn(move |context| {
            let running = started.fetch_or(own, Ordering::AcqRel) | own;
            if running == 0b111 {
                Poll::Ready(Ok(()))
            } else {
                context.waker().wake_by_ref();
                Poll::Pending
            }
        })
    };
    tokio::time::timeout(
        Duration::from_secs(1),
        InstallPlan::new(root.path().to_path_buf())
            .with_task(InstallTask::in_place(Vec::new(), installer(0b001)))
            .with_task(InstallTask::in_place(Vec::new(), installer(0b010)))
            .with_task(InstallTask::in_place(Vec::new(), installer(0b100)))
            .run(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(started.load(Ordering::Acquire), 0b111);
}

#[tokio::test]
async fn preparation_failure_settles_writers_before_restoring_metadata() {
    let root = tempfile::tempdir().unwrap();
    let metadata = root.path().join("manifest");
    fs::write(&metadata, "before").unwrap();
    let completed = Arc::new(AtomicBool::new(false));
    let pending = {
        let completed = Arc::clone(&completed);
        let metadata = metadata.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            fs::write(metadata, "late write").unwrap();
            completed.store(true, Ordering::Release);
            Ok(())
        }
    };
    let projection = Projection::new(root.path(), "environment");
    let resources = projection.resources.path().to_path_buf();
    let visible = projection.visible.clone();
    let error = InstallPlan::new(root.path().to_path_buf())
        .with_task(InstallTask::new(Vec::new(), async { Ok(vec![projection]) }))
        .with_task(InstallTask::in_place(vec![metadata.clone()], async { bail!("prepare failed") }))
        .with_task(InstallTask::in_place(vec![metadata.clone()], pending))
        .run()
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "prepare failed");
    assert_eq!(fs::read_to_string(metadata).unwrap(), "before");
    assert!(completed.load(Ordering::Acquire), "all writers must settle");
    assert!(!visible.exists(), "projection must not publish: {visible:?}");
    assert!(!resources.exists(), "unpublished resources must be cleaned: {resources:?}");
}

#[tokio::test]
async fn publishes_only_after_preparation_and_keeps_resources() {
    let root = tempfile::tempdir().unwrap();
    let projection = Projection::new(root.path(), "environment");
    let visible = projection.visible.clone();
    let resources = projection.resources.path().to_path_buf();
    let check = {
        let visible = visible.clone();
        async move {
            tokio::task::yield_now().await;
            assert!(!visible.exists(), "publication must wait: {visible:?}");
            Ok(())
        }
    };
    InstallPlan::new(root.path().to_path_buf())
        .with_task(InstallTask::new(Vec::new(), async { Ok(vec![projection]) }))
        .with_task(InstallTask::in_place(Vec::new(), check))
        .run()
        .await
        .unwrap();
    assert_eq!(fs::read_to_string(visible).unwrap(), resources.to_str().unwrap());
    assert!(resources.is_dir(), "published resources must survive: {resources:?}");
}

#[tokio::test]
async fn publication_failure_restores_all_attempted_projections_and_metadata() {
    let root = tempfile::tempdir().unwrap();
    let metadata = root.path().join("lockfile");
    fs::write(&metadata, "old lock").unwrap();
    let first = Projection::new(root.path(), "first");
    fs::write(&first.visible, "old environment").unwrap();
    let first_visible = first.visible.clone();
    let mut second = Projection::new(root.path(), "second");
    second.fail_publish = true;
    second.rollback_dependency =
        Some((first.visible.clone(), first.resources.path().to_path_buf()));
    let second_visible = second.visible.clone();
    let third = Projection::new(root.path(), "third");
    let third_visible = third.visible.clone();
    let resources =
        [&first, &second, &third].map(|projection| projection.resources.path().to_path_buf());
    let prepare = {
        let metadata = metadata.clone();
        async move {
            fs::write(metadata, "new lock").unwrap();
            Ok(vec![first])
        }
    };
    let error = InstallPlan::new(root.path().to_path_buf())
        .with_task(InstallTask::new(vec![metadata.clone()], prepare))
        .with_task(InstallTask::new(Vec::new(), async { Ok(vec![second, third]) }))
        .run()
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "publication failed");
    assert_eq!(fs::read_to_string(metadata).unwrap(), "old lock");
    assert_eq!(fs::read_to_string(first_visible).unwrap(), "old environment");
    assert!(!second_visible.exists(), "failed publisher must roll back: {second_visible:?}");
    assert!(!third_visible.exists(), "later publishers must not run: {third_visible:?}");
    for path in resources {
        assert!(!path.exists(), "rolled-back resources must be cleaned: {path:?}");
    }
}

#[tokio::test]
async fn rollback_failure_keeps_resources_and_still_restores_other_participants() {
    let root = tempfile::tempdir().unwrap();
    let metadata = root.path().join("lockfile");
    let first = Projection::new(root.path(), "first");
    let first_visible = first.visible.clone();
    let mut second = Projection::new(root.path(), "second");
    second.fail_publish = true;
    second.fail_rollback = true;
    let resources = second.resources.path().to_path_buf();
    let prepare = {
        let metadata = metadata.clone();
        async move {
            fs::write(metadata, "new lock").unwrap();
            Ok(vec![first, second])
        }
    };
    let error = InstallPlan::new(root.path().to_path_buf())
        .with_task(InstallTask::new(vec![metadata.clone()], prepare))
        .run()
        .await
        .unwrap_err();
    let error = format!("{error:?}");
    eprintln!("{error}");
    assert!(error.contains("publication failed"));
    assert!(error.contains("rollback failed"));
    assert!(!first_visible.exists(), "other rollback must still run: {first_visible:?}");
    assert!(!metadata.exists(), "metadata rollback must still run: {metadata:?}");
    assert!(resources.is_dir(), "failed rollback must retain resources: {resources:?}");
}
