use super::{
    AuthState, Config, Method, OneToken, ServiceExt, StatusCode, record, router_with_auth, signed,
    to_bytes,
};
use std::{
    collections::BTreeSet,
    sync::{Arc, atomic::Ordering},
};

#[tokio::test]
async fn a_large_roster_uses_one_targeted_bulk_account_lookup() {
    let directory = tempfile::tempdir().unwrap();
    let members: Vec<_> = (0..300)
        .map(|index| format!("member-{index}"))
        .chain(["retired".to_string()])
        .collect();
    let yaml = format!(
        "storage: '{}'\nauth:\n  admins: [alice]\nregistries:\n  local:\n    type: hosted\n    org: ''\n    teams:\n      readers:\n        name: readers\n        members: [{}]\ndefaultRegistry: local\n",
        directory
            .path()
            .join("storage")
            .display(),
        members.join(", "),
    );
    let path = directory.path().join("config.yaml");
    std::fs::write(&path, yaml).unwrap();
    let config = Config::from_yaml(&path, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    let mut accounts: BTreeSet<_> = members
        .iter()
        .filter(|id| id.as_str() != "retired")
        .cloned()
        .collect();
    accounts.extend(["alice".to_string(), "unrelated".to_string()]);
    let backend = Arc::new(OneToken {
        accounts,
        raw: "session".to_string(),
        record: record(false, &[]),
        account_reads: std::sync::atomic::AtomicUsize::default(),
        bulk_reads: std::sync::atomic::AtomicUsize::default(),
    });
    let auth = AuthState { users: Arc::clone(&backend) as _, tokens: Arc::clone(&backend) as _ };
    let app = router_with_auth(config, auth);
    let response = app
        .oneshot(signed(Method::GET, "/npm/-/team/acme/readers/user", "session"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let roster: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(roster.as_array().unwrap().len(), members.len());
    assert!(
        roster
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "deleted:retired"),
    );
    assert_eq!(backend.account_reads.load(Ordering::Relaxed), 1);
    assert_eq!(backend.bulk_reads.load(Ordering::Relaxed), 1);
}
