use pnpr::{
    UserBackend, UserStore,
    htpasswd_import::{HtpasswdImportOptions, import_htpasswd},
};
use pnpr_auth::{DELETED_USER_HASH, parse_htpasswd};
use pnpr_config::MaxUsers;

#[tokio::test]
async fn imports_preserve_retired_names_even_with_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("backup");
    let target = dir.path().join("htpasswd");
    let users = UserStore::open_with_cost(target.clone(), MaxUsers::Unlimited, 4).unwrap();
    users.create_user("former-member", "old-password").await.unwrap();
    std::fs::copy(&target, &source).unwrap();
    users.retire_user("former-member").await.unwrap();
    for overwrite in [false, true] {
        let options = HtpasswdImportOptions {
            source: source.clone(),
            target: Some(target.clone()),
            dry_run: false,
            overwrite,
        };
        let report = import_htpasswd(&options).unwrap();
        assert_eq!(report.merged, 0);
        assert_eq!(report.collisions, ["former-member (destination: retired, source: bcrypt)"]);
        assert!(users.is_retired("former-member").await.unwrap());
        assert!(users.add_or_login("former-member", "old-password").await.is_err());
    }
    let copied = dir.path().join("copied");
    let report = import_htpasswd(&HtpasswdImportOptions {
        source: target,
        target: Some(copied.clone()),
        dry_run: false,
        overwrite: true,
    })
    .unwrap();
    assert_eq!(report.total_read, 1);
    assert_eq!(report.merged, 1);
    assert_eq!(
        parse_htpasswd(&std::fs::read_to_string(copied).unwrap()).unwrap()["former-member"],
        DELETED_USER_HASH,
    );
}
