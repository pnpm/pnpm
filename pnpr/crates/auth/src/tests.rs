use super::{
    AccountLifecycle, AccountStatusOutcome, NewToken, PasswordChange, PasswordCollisions,
    PasswordImport, SqliteAuth, TokenBackend, TokenKind, UpsertOutcome, UserBackend, UserCreation,
    UserRemoval, identify, parse_htpasswd, sha256_hex,
};
use crate::validation::{
    MAX_USERNAME_CHARS, token_timestamp_from_sql, token_timestamp_to_sql, validate_username,
};
use pnpr_config::MaxUsers;
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::sync::Barrier;

/// Tests run with cost 4 (the bcrypt crate's minimum sane value)
/// so per-test wall-clock stays in the single-digit ms range.
/// Production paths use [`DEFAULT_BCRYPT_COST`].
const TEST_COST: u32 = 4;

const INVALID_USERNAMES: &[&str] = &[
    "",
    " alice",
    "alice ",
    "#alice",
    "alice:bob",
    "alice\nbob",
    "alice\rbob",
    "alice\0bob",
    "alice\u{7f}bob",
];

fn test_auth(max_users: MaxUsers) -> SqliteAuth {
    SqliteAuth::in_memory(max_users).unwrap().with_bcrypt_cost(TEST_COST)
}

fn open_auth(path: &Path) -> SqliteAuth {
    SqliteAuth::open(path, MaxUsers::Unlimited).unwrap().with_bcrypt_cost(TEST_COST)
}

/// Insert a token row the backend did not mint, for rows written by another
/// pnpr version or damaged by hand.
fn insert_raw_token(path: &Path, token_hash: &str, user_id: &str, extra: (&str, &str)) {
    let (column, value) = extra;
    rusqlite::Connection::open(path)
        .unwrap()
        .execute(
            &format!(
                "INSERT INTO tokens (token_hash, user_id, created_at, last_used_at, {column})
                 VALUES (?1, ?2, -1, -42, ?3)",
            ),
            rusqlite::params![token_hash, user_id, value],
        )
        .unwrap();
}

#[tokio::test]
async fn invalid_usernames_are_rejected_before_anything_is_stored() {
    let auth = test_auth(MaxUsers::Unlimited);
    for username in INVALID_USERNAMES {
        let err = auth.add_or_login(username, "secret123").await.unwrap_err();
        assert_eq!(
            err.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "expected {username:?} to be rejected",
        );
    }
    assert!(
        auth.list_users().await
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn adduser_creates_then_validates() {
    let auth = test_auth(MaxUsers::Unlimited);
    let outcome = auth.add_or_login("alice", "secret123").await.unwrap();
    assert!(matches!(outcome, (UpsertOutcome::Created, _)));
    assert_eq!(Some(outcome.1), auth.account_id("alice").await.unwrap());

    let outcome = auth.add_or_login("alice", "secret123").await.unwrap();
    assert!(matches!(outcome, (UpsertOutcome::LoggedIn, _)));
    assert_eq!(Some(outcome.1), auth.account_id("alice").await.unwrap());

    let err = auth.add_or_login("alice", "incorrect").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_registrations_of_one_name_create_one_account() {
    // Higher than TEST_COST so hashing lasts long enough for both tasks to
    // clear the initial missing-user check before either writes.
    let auth = Arc::new(SqliteAuth::in_memory(MaxUsers::Unlimited).unwrap().with_bcrypt_cost(8));
    let barrier = Arc::new(Barrier::new(3));
    let spawn_adduser = |password: &'static str| {
        let auth = Arc::clone(&auth);
        let barrier = Arc::clone(&barrier);
        tokio::spawn(async move {
            barrier.wait().await;
            auth.add_or_login("alice", password).await
        })
    };
    let add_a = spawn_adduser("password-a");
    let add_b = spawn_adduser("password-b");
    barrier.wait().await;
    let results = [add_a.await.unwrap(), add_b.await.unwrap()];

    let created = results
        .iter()
        .filter(|result| matches!(result, Ok((UpsertOutcome::Created, _))))
        .count();
    let unauthorized = results
        .iter()
        .filter(|result| {
            result
                .as_ref()
                .is_err_and(|err| err.status_code() == axum::http::StatusCode::UNAUTHORIZED)
        })
        .count();
    assert_eq!(created, 1, "exactly one concurrent adduser should create the account");
    assert_eq!(unauthorized, 1, "the losing registration must be rejected");
}

#[tokio::test]
async fn accounts_and_tokens_persist_in_the_database_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("metadata.sqlite");
    let auth = open_auth(&path);
    let (_, id) = auth.add_or_login("alice", "secret123").await.unwrap();
    let raw = auth.issue(&id).await.unwrap();
    drop(auth);

    let reopened = open_auth(&path);
    let outcome = reopened.add_or_login("alice", "secret123").await.unwrap();
    assert_eq!(outcome, (UpsertOutcome::LoggedIn, id.clone()));
    assert_eq!(reopened.lookup(&raw).await.unwrap(), Some(id));

    let stored: (String, String, String) = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT u.credential_kind, u.password_hash, t.token_hash
             FROM users u JOIN tokens t ON t.user_id = u.id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(stored.0, "bcrypt");
    assert!(stored.1.starts_with("$2y$"));
    assert_eq!(stored.2, sha256_hex(raw.as_bytes()), "only the token hash is stored");
}

/// A revocation from another process, such as `pnpr add-user` resetting a
/// password next to a running server, takes effect on the next lookup.
#[tokio::test]
async fn a_revocation_through_another_connection_is_seen_at_once() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("metadata.sqlite");
    let server = open_auth(&path);
    let (_, id) = server.add_or_login("alice", "secret123").await.unwrap();
    let raw = server.issue(&id).await.unwrap();
    assert_eq!(server.lookup(&raw).await.unwrap(), Some(id.clone()));

    let cli = open_auth(&path);
    assert_eq!(cli.revoke_all_for_user(&id).await.unwrap(), 1);
    assert_eq!(server.lookup(&raw).await.unwrap(), None);
}

#[tokio::test]
async fn registration_cap_and_disabled_registration() {
    let disabled = test_auth(MaxUsers::Disabled);
    let err = disabled.add_or_login("alice", "secret123").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::UNAUTHORIZED);

    let capped = test_auth(MaxUsers::Limited(2));
    capped.add_or_login("alice", "password-x").await.unwrap();
    capped.add_or_login("bob", "password-x").await.unwrap();
    let err = capped.add_or_login("carol", "password-x").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::FORBIDDEN);
    // Existing users may still log in once the cap is hit.
    capped.add_or_login("alice", "password-x").await.unwrap();
}

#[tokio::test]
async fn deactivated_accounts_keep_their_registration_slot_until_deleted() {
    let auth = test_auth(MaxUsers::Limited(1));
    let (_, id) = auth.add_or_login("alice", "secret123").await.unwrap();
    let raw = auth.issue(&id).await.unwrap();
    assert_eq!(
        auth.apply_account_status(&id, 1, AccountLifecycle::Deactivated).await.unwrap(),
        AccountStatusOutcome::Applied,
    );
    assert!(
        auth.list_for_user(&id).await
            .unwrap()
            .is_empty(),
        "deactivation revokes the account's tokens",
    );
    assert!(
        auth.lookup(&raw).await
            .unwrap()
            .is_none(),
    );
    let capped = auth.add_or_login("bob", "secret123").await.unwrap_err();
    assert!(matches!(capped, pnpr_error::RegistryError::TooManyUsers { max: 1 }));
    let deprovisioned = auth.add_or_login("alice", "secret123").await.unwrap_err();
    assert_eq!(deprovisioned.status_code(), axum::http::StatusCode::FORBIDDEN);
    assert_eq!(
        auth.apply_account_status(&id, 1, AccountLifecycle::Active).await.unwrap(),
        AccountStatusOutcome::Superseded,
    );

    assert_eq!(auth.delete_user(&id).await.unwrap(), UserRemoval::Removed);
    assert!(matches!(
        auth.add_or_login("bob", "secret123").await.unwrap(),
        (UpsertOutcome::Created, _),
    ));
}

#[tokio::test]
async fn sqlite_auth_supports_the_admin_operations() {
    assert_admin_round_trip(&test_auth(MaxUsers::Unlimited)).await;
}

#[tokio::test]
async fn admin_created_users_bypass_disabled_registration() {
    let auth = test_auth(MaxUsers::Disabled);
    assert!(matches!(
        auth.create_user("alice", "secret123").await.unwrap(),
        UserCreation::Created(_),
    ));
    assert_eq!(auth.list_users().await.unwrap(), ["alice"]);
}

#[tokio::test]
async fn look_alike_names_are_refused_and_renames_keep_ids() {
    let auth = test_auth(MaxUsers::Unlimited);
    let alice = auth.provision_user("alice").await.unwrap();
    let err = auth.add_or_login("ALICE", "secret123").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
    let err = auth.provision_user("alice").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::CONFLICT);

    auth.rename_user(&alice, "Alice").await.unwrap();
    assert_eq!(auth.account_id("Alice").await.unwrap(), Some(alice.clone()));
    let login = auth
        .account_login(&alice)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(login.revision, 1);
    assert_eq!(
        auth.rename_user_if_current(&alice, 0, "alice2").await.unwrap(),
        super::UserRename::CurrentChanged,
    );
    assert_eq!(
        auth.rename_user_if_current(&alice, 1, "alice2").await.unwrap(),
        super::UserRename::Renamed(2),
    );

    let bob = auth.provision_user("bob").await.unwrap();
    let err = auth.rename_user(&bob, "alice2").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::CONFLICT);

    assert_eq!(auth.delete_user(&alice).await.unwrap(), UserRemoval::Removed);
    assert_eq!(auth.account_status(&alice).await.unwrap(), Some(super::AccountStatus::Deleted));
    let reused = auth.provision_user("alice2").await.unwrap();
    assert_ne!(reused, alice, "a deleted account's ID is never reassigned");
}

#[tokio::test]
async fn tokens_round_trip_and_carry_their_restrictions() {
    let auth = test_auth(MaxUsers::Unlimited);
    let alice = auth.provision_user("alice").await.unwrap();
    let (raw, record) = auth
        .issue_token(
            &alice,
            NewToken::new()
                .with_readonly(true)
                .with_cidr_whitelist(vec!["127.0.0.1/32".to_string()])
                .with_name(Some("ci-token".to_string()))
                .with_expires_at(Some(4_000_000_000_000))
                .with_kind(Some(TokenKind::Granular)),
        )
        .await
        .unwrap();
    assert!(record.readonly);
    assert_ne!(raw, auth.issue(&alice).await.unwrap(), "every issue mints a fresh token");

    let looked_up = auth
        .lookup_record(&raw)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(looked_up.user_id, alice);
    assert_eq!(looked_up.owner_name(), Some("alice"));
    assert!(looked_up.readonly);
    assert_eq!(looked_up.cidr_whitelist, vec!["127.0.0.1/32".to_string()]);
    assert_eq!(looked_up.name.as_deref(), Some("ci-token"));
    assert_eq!(looked_up.expires_at, Some(4_000_000_000_000));
    assert_eq!(looked_up.kind, Some(TokenKind::Granular));
    assert!(
        auth.lookup("not-a-token").await
            .unwrap()
            .is_none(),
    );

    auth.touch(&raw, looked_up.last_used_at + 100).await
        .unwrap();
    let touched = auth
        .find_by_key(&sha256_hex(raw.as_bytes()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(touched.last_used_at, looked_up.last_used_at + 100);
}

#[tokio::test]
async fn expired_tokens_do_not_authenticate_and_are_deleted() {
    let auth = test_auth(MaxUsers::Unlimited);
    let alice = auth.provision_user("alice").await.unwrap();
    let mut issued = Vec::new();
    for expires_at in [Some(999), Some(1_000), Some(4_000_000_000_000), None] {
        let (raw, _) = auth
            .issue_token(&alice, NewToken::new().with_expires_at(expires_at))
            .await
            .unwrap();
        issued.push(raw);
    }
    assert!(
        auth.lookup(&issued[0]).await
            .unwrap()
            .is_none(),
    );
    assert!(
        auth.lookup_record(&issued[1]).await
            .unwrap()
            .is_none(),
    );

    assert_eq!(auth.delete_expired(1_000).await.unwrap(), 2);
    assert_eq!(auth.delete_expired(1_000).await.unwrap(), 0);
    assert!(
        auth.find_by_key(&sha256_hex(issued[1].as_bytes())).await
            .unwrap()
            .is_none(),
    );
    assert!(
        auth.lookup(&issued[2]).await
            .unwrap()
            .is_some(),
    );
    assert!(
        auth.lookup(&issued[3]).await
            .unwrap()
            .is_some(),
    );
}

#[tokio::test]
async fn tokens_of_a_removed_owner_stop_authenticating_but_stay_listed() {
    let auth = test_auth(MaxUsers::Unlimited);
    let alice = auth.provision_user("alice").await.unwrap();
    let bob = auth.provision_user("bob").await.unwrap();
    let token = auth.issue(&alice).await.unwrap();
    let revoked = auth.issue(&alice).await.unwrap();
    auth.issue(&bob).await.unwrap();
    assert!(
        auth.revoke_by_key(&sha256_hex(revoked.as_bytes())).await
            .unwrap()
            .is_some(),
    );
    assert_eq!(
        auth.list_for_user(&alice).await
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(
        auth.list_for_user(&bob).await
            .unwrap()
            .len(),
        1,
    );

    auth.delete_user(&alice).await.unwrap();
    assert!(
        auth.lookup(&token).await
            .unwrap()
            .is_none(),
    );
    assert!(
        auth.lookup_record(&token).await
            .unwrap()
            .is_none(),
    );
    assert_eq!(
        auth.list_for_user(&alice).await
            .unwrap()
            .len(),
        1,
    );
}

#[tokio::test]
async fn identify_recognizes_bearer_in_any_case_and_ignores_basic() {
    let auth = test_auth(MaxUsers::Unlimited);
    let alice = auth.provision_user("alice").await.unwrap();
    let token = auth.issue(&alice).await.unwrap();

    for scheme in ["Bearer", "bearer", "BEARER", "BeArEr"] {
        let header = format!("{scheme} {token}");
        assert_eq!(
            identify(Some(&header), &auth).await.unwrap(),
            Some(alice.clone()),
            "Bearer scheme {scheme:?} should be recognized",
        );
    }
    // Basic credentials are not accepted on requests, so request handling
    // never pays a bcrypt.
    for scheme in ["Basic", "basic", "BASIC", "bAsIc"] {
        let header = format!("{scheme} YWxpY2U6c2VjcmV0");
        assert!(identify(Some(&header), &auth).await.unwrap().is_none());
    }
    assert!(identify(None, &auth).await.unwrap().is_none());
    assert!(identify(Some("Bearer total-nonsense"), &auth).await.unwrap().is_none());
}

#[tokio::test]
async fn stored_token_rows_are_read_strictly() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("metadata.sqlite");
    let auth = open_auth(&path);
    let alice = auth.provision_user("alice").await.unwrap();

    let legacy_raw = "0123456789abcdef0123456789abcdef";
    insert_raw_token(&path, &sha256_hex(legacy_raw.as_bytes()), &alice, ("readonly", "0"));
    let record = auth
        .lookup_record(legacy_raw)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.user_id, alice);
    assert_eq!((record.created_at, record.last_used_at), (0, 0));

    let bad_cidr = "bad-cidr-token";
    insert_raw_token(&path, &sha256_hex(bad_cidr.as_bytes()), &alice, ("cidr_whitelist", "{ x"));
    let message = auth
        .lookup_record(bad_cidr)
        .await
        .unwrap_err()
        .to_string();
    assert!(message.contains("cidr_whitelist"), "{message}");

    let bad_kind = "bad-kind-token";
    insert_raw_token(&path, &sha256_hex(bad_kind.as_bytes()), &alice, ("kind", "custom"));
    let message = auth
        .lookup_record(bad_kind)
        .await
        .unwrap_err()
        .to_string();
    assert!(message.contains("kind"), "{message}");
}

#[tokio::test]
async fn sha1_user_authenticates_and_upgrades_to_bcrypt() {
    let auth = test_auth(MaxUsers::Unlimited);
    // "test-password" hashed with SHA-1 in Apache/Verdaccio format.
    let hashes = HashMap::from([(
        "verdaccio_user".to_string(),
        "{SHA}eJy+BAeECxwgQcszRS/2Dxm/WMw=".to_string(),
    )]);
    let options =
        PasswordImport { collisions: PasswordCollisions::Keep, mode: crate::ImportMode::Apply };
    auth.import_password_hashes(hashes, options).await.unwrap();
    let id = auth
        .account_id("verdaccio_user")
        .await
        .unwrap()
        .unwrap();

    let err = auth.add_or_login("verdaccio_user", "incorrect").await.unwrap_err();
    assert!(matches!(err, pnpr_error::RegistryError::Unauthenticated { .. }));
    let outcome = auth.add_or_login("verdaccio_user", "test-password").await.unwrap();
    assert_eq!(outcome, (UpsertOutcome::LoggedIn, id.clone()));
    let credential = auth
        .credential("verdaccio_user")
        .await
        .unwrap()
        .unwrap();
    assert!(
        credential
            .password_hash()
            .unwrap()
            .starts_with("$2y$"),
    );
    let outcome = auth.add_or_login("verdaccio_user", "test-password").await.unwrap();
    assert_eq!(outcome, (UpsertOutcome::LoggedIn, id));
}

#[tokio::test]
async fn password_imports_keep_or_overwrite_collisions_and_dry_runs_write_nothing() {
    let auth = test_auth(MaxUsers::Disabled);
    let UserCreation::Created(alice) = auth.create_user("alice", "secret123").await.unwrap() else {
        panic!("expected alice to be created");
    };
    let bob_hash = bcrypt::hash("bob-password", TEST_COST).unwrap();
    let alice_hash = bcrypt::hash("new-password", TEST_COST).unwrap();
    let hashes =
        HashMap::from([("alice".to_string(), alice_hash.clone()), ("bob".to_string(), bob_hash)]);

    let dry_run = PasswordImport {
        collisions: PasswordCollisions::Overwrite,
        mode: crate::ImportMode::Preview,
    };
    let imported = auth.import_password_hashes(hashes.clone(), dry_run).await.unwrap();
    assert_eq!(imported.merged, 2);
    assert_eq!(auth.list_users().await.unwrap(), ["alice"]);

    let keep =
        PasswordImport { collisions: PasswordCollisions::Keep, mode: crate::ImportMode::Apply };
    let imported = auth.import_password_hashes(hashes.clone(), keep).await.unwrap();
    assert_eq!((imported.merged, imported.collisions), (1, vec!["alice".to_string()]));
    assert_eq!(auth.list_users().await.unwrap(), ["alice", "bob"]);
    auth.add_or_login("alice", "secret123").await.unwrap();
    let old_token = auth.issue(&alice).await.unwrap();

    let overwrite = PasswordImport {
        collisions: PasswordCollisions::Overwrite,
        mode: crate::ImportMode::Apply,
    };
    let imported = auth.import_password_hashes(hashes, overwrite).await.unwrap();
    assert_eq!(imported.merged, 1);
    let outcome = auth.add_or_login("alice", "new-password").await.unwrap();
    assert_eq!(outcome, (UpsertOutcome::LoggedIn, alice), "an overwrite keeps the account ID");
    assert!(
        auth.lookup(&old_token).await
            .unwrap()
            .is_none(),
        "an overwrite revokes the tokens issued under the old password",
    );

    let look_alike = HashMap::from([("ALICE".to_string(), alice_hash)]);
    let err = auth.import_password_hashes(look_alike, keep).await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_preview_copies_the_accounts_without_touching_the_database() {
    let root = tempfile::tempdir().unwrap();
    // URI metacharacters in the directory name.
    let dir = root.path().join("a#b%c");
    std::fs::create_dir(&dir).unwrap();
    let missing = dir.join("missing").join("metadata.sqlite");
    let preview = SqliteAuth::preview_of(&missing, MaxUsers::Unlimited).unwrap();
    assert!(
        preview
            .list_users()
            .await
            .unwrap()
            .is_empty(),
    );
    assert!(!missing.parent().unwrap().exists());

    let path = dir.join("metadata.sqlite");
    let auth = open_auth(&path);
    let (_, alice) = auth.add_or_login("alice", "secret123").await.unwrap();
    drop(auth);
    let before = std::fs::read(&path).unwrap();
    let preview =
        SqliteAuth::preview_of(&path, MaxUsers::Unlimited).unwrap().with_bcrypt_cost(TEST_COST);
    let hashes = HashMap::from([("alice".to_string(), bcrypt::hash("other-password", 4).unwrap())]);
    let options =
        PasswordImport { collisions: PasswordCollisions::Keep, mode: crate::ImportMode::Preview };
    let imported = preview.import_password_hashes(hashes, options).await.unwrap();
    assert_eq!(imported.collisions, ["alice"]);
    assert_eq!(preview.account_id("alice").await.unwrap(), Some(alice));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files, ["metadata.sqlite"], "a preview creates no files beside the database");

    // A running server keeps commits in its `-wal` file. The preview reads them.
    let server = open_auth(&path);
    server.add_or_login("bob", "secret123").await.unwrap();
    let preview = SqliteAuth::preview_of(&path, MaxUsers::Unlimited).unwrap();
    assert_eq!(preview.list_users().await.unwrap(), ["alice", "bob"]);
}

#[cfg(unix)]
#[tokio::test]
async fn only_the_owner_can_read_the_database() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let created = tmp.path().join("created.sqlite");
    let existing = tmp.path().join("existing.sqlite");
    drop(rusqlite::Connection::open(&existing).unwrap());
    std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o644)).unwrap();
    for path in [&created, &existing] {
        let auth = open_auth(path);
        auth.add_or_login("alice", "secret123").await.unwrap();
        for suffix in ["", "-wal", "-shm"] {
            let mut file = path.as_os_str().to_os_string();
            file.push(suffix);
            let mode = std::fs::metadata(&file)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{}", file.to_string_lossy());
        }
    }
}

#[tokio::test]
async fn readiness_fails_without_the_tokens_table() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("metadata.sqlite");
    let auth = open_auth(&path);
    TokenBackend::check_readiness(&auth).await.unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TABLE tokens")
        .unwrap();
    TokenBackend::check_readiness(&auth).await.unwrap_err();
}

#[tokio::test]
async fn opening_a_database_migrates_it_to_the_current_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("metadata.sqlite");
    drop(open_auth(&path));
    let version: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, super::CURRENT_SCHEMA_VERSION);
}

#[tokio::test]
async fn incompatible_and_newer_databases_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let unversioned = tmp.path().join("unversioned.sqlite");
    rusqlite::Connection::open(&unversioned)
        .unwrap()
        .execute_batch("CREATE TABLE tokens (token_hash TEXT PRIMARY KEY, username TEXT);")
        .unwrap();
    assert!(SqliteAuth::open(&unversioned, MaxUsers::Unlimited).is_err());

    let future = pnpr_sql::schema::SCHEMA_VERSION + 1;
    let newer = tmp.path().join("newer.sqlite");
    let conn = rusqlite::Connection::open(&newer).unwrap();
    conn.execute_batch(pnpr_sql::schema::MARKER_TABLE.sqlite).unwrap();
    for version in 1..=future {
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, 1)",
            [version],
        )
        .unwrap();
    }
    drop(conn);
    let err = SqliteAuth::open(&newer, MaxUsers::Unlimited).unwrap_err().to_string();
    assert!(err.contains(&format!("schema version {future}")), "{err}");
}

#[tokio::test]
async fn password_changes_report_missing_accounts() {
    let auth = test_auth(MaxUsers::Unlimited);
    assert_eq!(
        auth.set_password("no-such-id", "password-x").await.unwrap(),
        PasswordChange::NoSuchUser,
    );
    assert_eq!(auth.clear_password("no-such-id").await.unwrap(), PasswordChange::NoSuchUser);
}

#[test]
fn username_length_limit_matches_sql_schema() {
    let max = "a".repeat(MAX_USERNAME_CHARS);
    let too_long = "a".repeat(MAX_USERNAME_CHARS + 1);
    validate_username(&max).unwrap();
    let err = validate_username(&too_long).unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[test]
fn token_timestamp_from_sql_clamps_negative_values() {
    assert_eq!(token_timestamp_from_sql(-1), 0);
    assert_eq!(token_timestamp_from_sql(0), 0);
    assert_eq!(token_timestamp_from_sql(42), 42);
}

#[test]
fn token_timestamp_to_sql_saturates_overflow() {
    assert_eq!(token_timestamp_to_sql(42), 42);
    assert_eq!(token_timestamp_to_sql(u64::MAX), i64::MAX);
}

#[test]
fn username_validation_rejects_htpasswd_structural_characters() {
    for username in INVALID_USERNAMES {
        let err = validate_username(username).unwrap_err();
        assert_eq!(
            err.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "expected {username:?} to be rejected",
        );
    }
}

#[test]
fn username_validation_rejects_names_that_trim_differently() {
    for username in [" alice", "alice ", " alice "] {
        let err = validate_username(username).unwrap_err();
        assert_eq!(
            err.status_code(),
            axum::http::StatusCode::BAD_REQUEST,
            "expected {username:?} to be rejected",
        );
    }
}

#[test]
fn parse_htpasswd_accepts_blank_and_comment_lines() {
    let raw = "\n# comment\nalice:$2y$10$abcdef\n";
    let map = parse_htpasswd(raw).unwrap();
    assert_eq!(map.len(), 1);
    assert!(map.contains_key("alice"));
}

#[test]
fn parse_htpasswd_preserves_legacy_whitespace_normalization() {
    let map = parse_htpasswd(" alice :$2y$10$abcdef\n").unwrap();
    assert_eq!(map.get("alice").map(String::as_str), Some("$2y$10$abcdef"));
}

#[test]
fn parse_htpasswd_rejects_invalid_usernames() {
    for raw in [" #alice:$2y$10$abcdef\n", "alice\u{1}admin:$2y$10$abcdef\n"] {
        assert!(parse_htpasswd(raw).is_err(), "expected {raw:?} to be rejected");
    }
}

/// The admin operations every [`UserBackend`] supports, starting from an
/// empty store.
pub(crate) async fn assert_admin_round_trip(users: &dyn UserBackend) {
    for password in ["short", "ééééééé", &"a".repeat(72)] {
        let err = users.create_user("alice", password).await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
    }
    assert!(
        users
            .account_id("alice")
            .await
            .unwrap()
            .is_none(),
    );
    let UserCreation::Created(created_id) =
        users.create_user("alice", "first-password").await.unwrap()
    else {
        panic!("expected alice to be created");
    };
    assert_eq!(
        users.create_user("alice", "other-password").await.unwrap(),
        UserCreation::NameTaken,
    );
    assert_eq!(users.list_users().await.unwrap(), ["alice"]);

    let id = users
        .account_id("alice")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(id, created_id);
    let err = users.set_password(&id, "short").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
    users.add_or_login("alice", "first-password").await.unwrap();
    assert_eq!(users.set_password(&id, "second-password").await.unwrap(), PasswordChange::Changed);
    assert!(users.add_or_login("alice", "first-password").await.is_err());
    assert!(matches!(
        users.add_or_login("alice", "second-password").await.unwrap(),
        (UpsertOutcome::LoggedIn, _),
    ));
    assert_eq!(users.set_password("bob", "password-x").await.unwrap(), PasswordChange::NoSuchUser);

    assert_eq!(users.delete_user(&id).await.unwrap(), UserRemoval::Removed);
    assert_eq!(users.delete_user(&id).await.unwrap(), UserRemoval::NoSuchUser);
    assert!(
        users
            .list_users()
            .await
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn verify_bcrypt_handles_passwords_exceeding_72_bytes() {
    let password_71 = "a".repeat(71);
    let hash = super::hash_bcrypt(password_71.clone(), TEST_COST).await.unwrap();

    assert!(super::verify_bcrypt(password_71, hash.clone()).await.unwrap());

    // 72-byte password must safely evaluate to false without error or truncation-match.
    let password_72 = "a".repeat(72);
    assert!(!super::verify_bcrypt(password_72, hash).await.unwrap());
}

#[test]
fn parse_htpasswd_accepts_sha1_entries() {
    let raw = "alice:{SHA}qUqP5cyxm6YcTAhz05Hph5gvu9M=\nbob:$2y$04$somevalidhashhere12345678901234567890123456789012\n";
    let users = super::parse_htpasswd(raw).unwrap();
    assert_eq!(users.get("alice"), Some(&"{SHA}qUqP5cyxm6YcTAhz05Hph5gvu9M=".to_string()));
    assert!(users.contains_key("bob"));
}

#[test]
fn personal_tokens_use_pnpr_prefix_base62_payload_and_ieee_crc32() {
    use crate::token_store::format::{crc32_ieee, format_personal_token};

    assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);

    for byte in [0_u8, 1, 0x5A, 0xFF] {
        let token = format_personal_token([byte; 32]);
        assert_eq!(token.len(), super::PERSONAL_TOKEN_LEN);
        assert!(token.starts_with(super::PERSONAL_TOKEN_PREFIX));
        assert!(super::has_valid_personal_token_checksum(&token));
        let body = &token[super::PERSONAL_TOKEN_PREFIX.len()..];
        assert!(body.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert!(!token.starts_with("pnpr_oidc_"));
        assert!(!token.starts_with("pnpr_workload_"));
        assert!(!token.starts_with("pnpr_oci_"));
    }

    let valid = format_personal_token([0x42; 32]);
    for index in 0..valid.len() {
        let mut mutated = valid.clone().into_bytes();
        mutated[index] = if mutated[index] == b'a' { b'b' } else { b'a' };
        let mutated = String::from_utf8(mutated).unwrap();
        assert!(
            !super::has_valid_personal_token_checksum(&mutated),
            "single-byte mutation at {index} must fail checksum: {mutated}",
        );
    }
    assert!(!super::has_valid_personal_token_checksum("pnpr_short"));
    assert!(!super::has_valid_personal_token_checksum(&format!("{valid}0")));
}

#[cfg(feature = "backend-postgres")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires PNPR_TEST_POSTGRES_URL"]
async fn postgres_auth_uses_unified_schema_migrations_and_round_trips_tokens() {
    let base_url = std::env::var("PNPR_TEST_POSTGRES_URL").unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let db_name = format!(
        "auth_{}",
        tmp.path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(['.', '-'], "")
            .to_ascii_lowercase(),
    );
    let admin_pool = sqlx::PgPool::connect(&base_url).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {db_name}")))
        .execute(&admin_pool)
        .await
        .unwrap();
    let db_url = format!("{}/{db_name}", base_url.rsplit_once('/').unwrap().0);
    let settings = pnpr_config::SqlBackendSettings {
        url: db_url.clone(),
        max_connections: Some(2),
        timeout: std::time::Duration::from_secs(30),
        startup_timeout: std::time::Duration::from_secs(30),
    };
    let auth = super::PostgresAuth::connect(&settings, MaxUsers::Unlimited)
        .await
        .unwrap();
    let (_, user_id) = auth.add_or_login("alice", "secret123").await.unwrap();
    let (outcome, logged_in_id) = auth.add_or_login("alice", "secret123").await.unwrap();
    assert_eq!(outcome, UpsertOutcome::LoggedIn);
    assert_eq!(logged_in_id, user_id);
    let (_, _) = auth.add_or_login("zo\u{00e9}", "secret123").await.unwrap();
    let decomposed = auth.add_or_login("zoe\u{0301}", "secret123").await.unwrap_err();
    assert_eq!(decomposed.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(auth.account_id("zoe\u{0301}").await.unwrap(), None);
    assert_eq!(auth.list_users().await.unwrap(), ["alice", "zo\u{00e9}"]);
    let (raw, record) = auth
        .issue_token(&user_id, NewToken::new().with_readonly(true))
        .await
        .unwrap();
    assert!(record.readonly);
    let looked_up = auth
        .lookup_record(&raw)
        .await
        .unwrap()
        .unwrap();
    assert!(looked_up.readonly);
    assert_eq!(looked_up.user_id, user_id);
    drop(auth);

    let pool = sqlx::PgPool::connect(&db_url).await.unwrap();
    let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, super::CURRENT_SCHEMA_VERSION);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {db_name} WITH (FORCE)")))
        .execute(&admin_pool)
        .await
        .unwrap();
    admin_pool.close().await;
}

#[cfg(feature = "backend-mysql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires PNPR_TEST_MYSQL_URL"]
async fn mysql_auth_uses_unified_schema_migrations_and_round_trips_tokens() {
    let base_url = std::env::var("PNPR_TEST_MYSQL_URL").unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let db_name = format!(
        "auth_{}",
        tmp.path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace(['.', '-'], "")
            .to_ascii_lowercase(),
    );
    let admin_pool = sqlx::MySqlPool::connect(&base_url).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {db_name}")))
        .execute(&admin_pool)
        .await
        .unwrap();
    let db_url = format!("{}/{db_name}", base_url.rsplit_once('/').unwrap().0);
    let settings = pnpr_config::SqlBackendSettings {
        url: db_url.clone(),
        max_connections: Some(2),
        timeout: std::time::Duration::from_secs(30),
        startup_timeout: std::time::Duration::from_secs(30),
    };
    let auth = super::MysqlAuth::connect(&settings, MaxUsers::Unlimited).await.unwrap();
    let (_, user_id) = auth.add_or_login("alice", "secret123").await.unwrap();
    let (outcome, logged_in_id) = auth.add_or_login("alice", "secret123").await.unwrap();
    assert_eq!(outcome, UpsertOutcome::LoggedIn);
    assert_eq!(logged_in_id, user_id);
    let (_, _) = auth.add_or_login("zo\u{00e9}", "secret123").await.unwrap();
    let decomposed = auth.add_or_login("zoe\u{0301}", "secret123").await.unwrap_err();
    assert_eq!(decomposed.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(auth.account_id("zoe\u{0301}").await.unwrap(), None);
    assert_eq!(auth.list_users().await.unwrap(), ["alice", "zo\u{00e9}"]);
    let (raw, record) = auth
        .issue_token(&user_id, NewToken::new().with_readonly(true))
        .await
        .unwrap();
    assert!(record.readonly);
    let looked_up = auth
        .lookup_record(&raw)
        .await
        .unwrap()
        .unwrap();
    assert!(looked_up.readonly);
    assert_eq!(looked_up.user_id, user_id);
    drop(auth);

    let pool = sqlx::MySqlPool::connect(&db_url).await.unwrap();
    let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, super::CURRENT_SCHEMA_VERSION);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {db_name}")))
        .execute(&admin_pool)
        .await
        .unwrap();
    admin_pool.close().await;
}
