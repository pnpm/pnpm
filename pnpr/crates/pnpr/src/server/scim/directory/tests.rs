use super::ScimState;
use pnpr_auth::AuthState;
use pnpr_config::{Config, ScimConfig};
use pnpr_fixtures::MockConfigPresets;
use pnpr_storage::metadata::ScimStatus;
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

#[tokio::test]
async fn deactivation_holds_the_binding_lock_until_revocation_finishes() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut config =
        Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    config.identity.auth.scim =
        Some(ScimConfig { token: "scim-secret-0123456789abcdef0123456789".to_string() });
    let (state, _) = crate::server::state_and_router(config, AuthState::in_memory(), None).unwrap();
    let directory = Arc::new(ScimState::default());
    let mut status = ScimStatus {
        user_id: "example-account".to_string(),
        external_id: Some("subject-one".to_string()),
        active: true,
        deprovisions: 0,
        generation: 1,
    };
    directory.apply_statuses(&state, &[status.clone()]);
    let tracker = directory.tracker.lock().unwrap();
    status.active = false;
    status.deprovisions = 1;
    status.generation = 2;
    let updating = Arc::clone(&directory);
    let worker = thread::spawn(move || updating.apply_statuses(&state, &[status]));
    let deadline = Instant::now() + Duration::from_secs(5);
    let inactive = loop {
        if directory.active_by_id
            .read()
            .unwrap()
            .get("example-account")
            == Some(&false)
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        thread::sleep(Duration::from_millis(1));
    };
    let binding_locked = directory.subjects.try_read().is_err();
    drop(tracker);
    worker.join().unwrap();
    assert!(inactive, "deactivation must reach the revocation phase");
    assert!(binding_locked, "subject resolution must wait until revocation completes");
    assert!(directory.subjects.try_read().is_ok());
}
