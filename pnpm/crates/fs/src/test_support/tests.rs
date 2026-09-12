use super::{notify_attempt, with_retry_observer};
use std::panic::catch_unwind;
use tempfile::tempdir;

#[test]
fn observer_is_removed_when_its_callback_panics() {
    let root = tempdir().unwrap();
    let path = root.path().join("shim");
    let result = catch_unwind(|| {
        with_retry_observer(
            &path,
            |_| panic!("observer failure"),
            || notify_attempt(&path, &Ok(())),
        );
    });
    assert!(result.is_err(), "the observer must have run and panicked");
    with_retry_observer(&path, |_| {}, || notify_attempt(&path, &Ok(())));
}

#[test]
fn duplicate_registration_does_not_remove_the_original_observer() {
    let root = tempdir().unwrap();
    let path = root.path().join("shim");
    let (sender, receiver) = std::sync::mpsc::channel();
    with_retry_observer(
        &path,
        move |_| sender.send(()).unwrap(),
        || {
            let result = catch_unwind(|| with_retry_observer(&path, |_| {}, || {}));
            assert!(result.is_err(), "duplicate registration must be rejected");
            notify_attempt(&path, &Ok(()));
        },
    );
    assert_eq!(receiver.try_iter().count(), 1);
}
