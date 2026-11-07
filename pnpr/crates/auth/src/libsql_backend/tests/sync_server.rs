//! An embedded replica of the auth database against a local primary that
//! serves libsql's newer sync protocol, as Turso Cloud does.

use super::{Builder, LibsqlAuth, MaxUsers, TokenBackend, UpsertOutcome, UserBackend, UserRemoval};
use pnpr_config::LibsqlSettings;
use pnpr_libsql::testing::serve_primary;
use std::{net::SocketAddr, path::PathBuf, sync::atomic::Ordering, time::Duration};

fn replica_settings(
    address: SocketAddr,
    replica_path: PathBuf,
    sync_interval_secs: Option<u64>,
) -> LibsqlSettings {
    LibsqlSettings {
        url: format!("http://{address}"),
        auth_token: None,
        replica_path: Some(replica_path),
        sync_interval_secs,
        max_connections: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_replica_follows_a_primary_on_the_newer_sync_protocol() {
    let directory = tempfile::tempdir().unwrap();
    let (primary, address, server) = serve_primary(directory.path().join("primary.db")).await;
    let db = Builder::new_local(&primary.path).build().await.unwrap();
    let backend = LibsqlAuth::from_database(db, MaxUsers::Unlimited, 4).await.unwrap();
    backend.add_or_login("alice", "secret").await.unwrap();
    let alice_id = backend
        .account_id("alice")
        .await
        .unwrap()
        .unwrap();

    for (index, sync_interval_secs) in [Some(1), Some(0)].into_iter().enumerate() {
        let token = backend.issue(&alice_id).await.unwrap();
        let replica_path = directory
            .path()
            .join(format!("replica{index}"))
            .join("auth.db");
        let settings = replica_settings(address, replica_path, sync_interval_secs);
        let pipelines_before_connect = primary.round_trips.count();
        let replica = LibsqlAuth::connect(&settings, MaxUsers::Unlimited).await.unwrap();

        assert_eq!(
            primary.exports.load(Ordering::SeqCst),
            index + 1,
            "bootstrapped from the primary's export",
        );
        let pipelines_after_connect = primary.round_trips.count();
        assert!(
            pipelines_after_connect > pipelines_before_connect,
            "schema setup writes go to the primary",
        );

        let owner = replica.lookup(&token).await.unwrap();
        assert_eq!(owner.as_deref(), Some(alice_id.as_str()));
        let hash = replica.stored_hash("alice").await.unwrap();
        assert!(hash.is_some());
        assert_eq!(
            primary.round_trips.count(),
            pipelines_after_connect,
            "reads are served from the local replica",
        );

        let issued = replica.issue(&alice_id).await.unwrap();
        assert!(primary.round_trips.count() > pipelines_after_connect, "writes go to the primary");
        let owner = backend.lookup(&issued).await.unwrap();
        assert_eq!(owner.as_deref(), Some(alice_id.as_str()));
        let owner = replica.lookup(&issued).await.unwrap();
        assert_eq!(owner.as_deref(), Some(alice_id.as_str()), "own write visible");

        let mut tokens = vec![token, issued];
        if sync_interval_secs == Some(1) {
            let fresh = backend.issue(&alice_id).await.unwrap();
            let owner = wait_for_token(&replica, &fresh).await;
            assert_eq!(
                owner.as_deref(),
                Some(alice_id.as_str()),
                "background sync pulls the write",
            );
            tokens.push(fresh);
        }

        let revoked = replica.revoke_all_for_user(&alice_id).await.unwrap();
        assert_eq!(revoked, tokens.len());
        for token in &tokens {
            assert_eq!(backend.lookup(token).await.unwrap(), None);
            assert_eq!(replica.lookup(token).await.unwrap(), None, "own bulk revoke visible");
        }
    }
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_replica_sets_up_the_schema_on_an_empty_primary() {
    let directory = tempfile::tempdir().unwrap();
    let (primary, address, server) = serve_primary(directory.path().join("primary.db")).await;
    let replica_path = directory
        .path()
        .join("replica")
        .join("auth.db");
    let settings = replica_settings(address, replica_path, None);
    let replica = LibsqlAuth::connect(&settings, MaxUsers::Limited(2)).await.unwrap();

    let (outcome, carol_id) = replica.add_or_login("carol", "secret").await.unwrap();
    assert!(matches!(outcome, UpsertOutcome::Created));
    let token = replica.issue(&carol_id).await.unwrap();
    let owner = replica.lookup(&token).await.unwrap();
    assert_eq!(owner.as_deref(), Some(carol_id.as_str()));

    let mut rows = primary.conn
        .query("SELECT value FROM auth_counters WHERE name = 'users'", ())
        .await
        .unwrap();
    let counter: i64 = rows
        .next()
        .await
        .unwrap()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(counter, 1, "the capped registration claimed a slot on the primary");
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bulk_revocation_refreshes_a_replica_whose_tokens_went_elsewhere() {
    let directory = tempfile::tempdir().unwrap();
    let (primary, address, server) = serve_primary(directory.path().join("primary.db")).await;
    let db = Builder::new_local(&primary.path).build().await.unwrap();
    let backend = LibsqlAuth::from_database(db, MaxUsers::Unlimited, 4).await.unwrap();
    backend.add_or_login("alice", "secret").await.unwrap();
    let alice_id = backend
        .account_id("alice")
        .await
        .unwrap()
        .unwrap();
    let replica_path = directory
        .path()
        .join("replica")
        .join("auth.db");
    let settings = replica_settings(address, replica_path, Some(0));
    let replica = LibsqlAuth::connect(&settings, MaxUsers::Unlimited).await.unwrap();

    let stale = backend.issue(&alice_id).await.unwrap();
    replica.issue(&alice_id).await.unwrap();
    assert_eq!(
        replica
            .lookup(&stale)
            .await
            .unwrap()
            .as_deref(),
        Some(alice_id.as_str()),
    );
    assert_eq!(backend.revoke_all_for_user(&alice_id).await.unwrap(), 2);

    let revoked = replica.revoke_all_for_user(&alice_id).await.unwrap();
    assert_eq!(revoked, 0, "the primary had nothing left to revoke");
    assert_eq!(replica.lookup(&stale).await.unwrap(), None, "the replica caught up");
    server.abort();
}

/// Poll `replica` for `token` until it is visible or the wait runs out.
async fn wait_for_token(replica: &LibsqlAuth, token: &str) -> Option<String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let owner = replica.lookup(token).await.unwrap();
        if owner.is_some() || tokio::time::Instant::now() > deadline {
            return owner;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retiring_a_passwordless_account_syncs_its_identity() {
    let directory = tempfile::tempdir().unwrap();
    let (_primary, address, server) = serve_primary(directory.path().join("primary.db")).await;
    let settings = replica_settings(address, directory.path().join("replica/auth.db"), Some(0));
    let replica = LibsqlAuth::connect(&settings, MaxUsers::Unlimited).await.unwrap();
    let id = replica.provision_user("oidc-member").await.unwrap();
    assert_eq!(replica.delete_user(&id).await.unwrap(), UserRemoval::Removed);
    assert_eq!(replica.account_name(&id).await.unwrap(), None);
    let replacement = replica.provision_user("oidc-member").await.unwrap();
    assert_ne!(id, replacement);
    assert_eq!(replica.account_id("oidc-member").await.unwrap(), Some(replacement));
    server.abort();
}
