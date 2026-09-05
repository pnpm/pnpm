use super::run_installers;
use std::{
    future::poll_fn,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
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
        run_installers(vec![
            Box::pin(installer(0b001)),
            Box::pin(installer(0b010)),
            Box::pin(installer(0b100)),
        ]),
    )
    .await
    .expect("every installer must start without waiting for another to finish")
    .unwrap();
    assert_eq!(started.load(Ordering::Acquire), 0b111);
}
