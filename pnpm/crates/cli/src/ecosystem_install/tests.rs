use super::EcosystemInstallCoordinator;
use std::{
    future::poll_fn,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    task::Poll,
    time::Duration,
};

#[tokio::test]
async fn polls_ecosystem_installers_concurrently() {
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
        EcosystemInstallCoordinator::new(installer(0b001))
            .with_install(installer(0b010))
            .with_install(installer(0b100))
            .run(),
    )
    .await
    .expect("every installer must start without waiting for another to finish")
    .unwrap();
    assert_eq!(started.load(Ordering::Acquire), 0b111);
}

#[tokio::test]
async fn waits_for_every_installer_after_an_error() {
    let completed = Arc::new(AtomicBool::new(false));
    let remaining_install = {
        let completed = Arc::clone(&completed);
        async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            completed.store(true, Ordering::Release);
            Ok(())
        }
    };

    let result = EcosystemInstallCoordinator::new(async { Err(miette::miette!("failed")) })
        .with_install(remaining_install)
        .run()
        .await;

    assert_eq!(result.unwrap_err().to_string(), "failed");
    assert!(completed.load(Ordering::Acquire));
}
