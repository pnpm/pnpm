use super::{Host, PackError, api, fixture, json};
use crate::{PackOutputLocks, pack_output_path};
use pnpm_reporter::SilentReporter;
use tempfile::tempdir;

#[test]
fn base_dir_containing_placeholders_is_not_substituted_by_out_template() {
    let outer = tempdir().unwrap();
    let base_dir = outer.path().join("dir-%s-%v-ws");
    let pkg_dir = base_dir.join("packages").join("pkg-a");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join("package.json"),
        serde_json::to_string(&json!({ "name": "pkg-a", "version": "1.2.3" })).unwrap(),
    )
    .unwrap();

    let (_unused_dir, mut opts) = fixture(&json!({ "name": "pkg-a", "version": "1.2.3" }));
    opts.dir = pkg_dir;
    opts.output.base_dir = Some(base_dir.clone());
    opts.output.out = Some("sub/%s-%v.tgz".to_string());

    let result = api::<SilentReporter, Host>(&opts).unwrap();
    let expected = base_dir.join("sub").join("pkg-a-1.2.3.tgz");
    assert!(expected.is_file(), "tarball must be written under literal base_dir");
    assert_eq!(result.tarball_path, expected.display().to_string());
    assert!(
        !outer
            .path()
            .join("dir-pkg-a-1.2.3-ws")
            .exists(),
        "parent path placeholders must not be expanded",
    );
}

#[test]
fn pack_output_path_preserves_placeholders_in_base_dir() {
    let outer = tempdir().unwrap();
    let base_dir = pnpm_fs::resolve_path_with_symlinks(outer.path()).unwrap().join("dir-%s-%v-ws");
    let resolved = pack_output_path(&base_dir, Some("%s-%v.tgz"), None, "@scope/pkg", "2.0.0+b1")
        .expect("resolve output path");
    assert_eq!(resolved, base_dir.join("scope-pkg-2.0.0.tgz"));
}

#[test]
fn rejects_dot_and_dotdot_and_metadata_only_versions() {
    for bad_version in
        [".", "..", "+build", "C:", "C:1.0.0", "1.0.0/../../escape", r"1.0.0\..\..\escape"]
    {
        let (_dir, opts) = fixture(&json!({ "name": "pkg", "version": bad_version }));
        let error = api::<SilentReporter, Host>(&opts).unwrap_err();
        assert!(
            matches!(error, PackError::InvalidPackageVersion { .. }),
            "expected InvalidPackageVersion for {bad_version:?}, got {error:?}",
        );
    }
}

#[cfg(unix)]
#[test]
fn destination_with_symlink_and_parent_dir_follows_symlink_target() {
    let outer = tempdir().unwrap();
    let real_parent = outer.path().join("real-parent");
    let real_sub = real_parent.join("sub");
    let pkg_dir = outer.path().join("pkg");
    std::fs::create_dir_all(&real_sub).unwrap();
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::os::unix::fs::symlink(&real_sub, pkg_dir.join("link")).unwrap();
    std::fs::write(
        pkg_dir.join("package.json"),
        serde_json::to_string(&json!({ "name": "pkg-a", "version": "1.2.3" })).unwrap(),
    )
    .unwrap();

    let (_unused_dir, mut opts) = fixture(&json!({ "name": "pkg-a", "version": "1.2.3" }));
    opts.dir = pkg_dir.clone();
    opts.output.destination = Some("link/../tarballs".to_string());

    api::<SilentReporter, Host>(&opts).unwrap();
    assert!(
        real_parent
            .join("tarballs")
            .join("pkg-a-1.2.3.tgz")
            .is_file(),
        "tarball must be written under the parent of the symlink target",
    );
    assert!(
        !pkg_dir.join("tarballs").exists(),
        "destination must not collapse symlink/.. lexically before filesystem resolution",
    );
}

#[cfg(unix)]
#[test]
fn pack_output_path_resolves_symlink_before_parent_dir() {
    let outer = tempdir().unwrap();
    let real_parent = outer.path().join("real-parent");
    let real_sub = real_parent.join("sub");
    let ws_dir = outer.path().join("ws");
    std::fs::create_dir_all(&real_sub).unwrap();
    std::fs::create_dir_all(&ws_dir).unwrap();
    std::os::unix::fs::symlink(&real_sub, ws_dir.join("link")).unwrap();

    let via_symlink = pack_output_path(&ws_dir, None, Some("link/../tarballs"), "pkg-a", "1.0.0")
        .expect("resolve symlinked output path");
    let direct = pack_output_path(&real_parent, None, Some("tarballs"), "pkg-a", "1.0.0")
        .expect("resolve direct output path");
    assert_eq!(via_symlink, direct);
}

#[cfg(unix)]
#[tokio::test]
async fn pack_output_locks_serialize_symlink_and_direct_paths() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let outer = tempdir().unwrap();
    let real_parent = outer.path().join("real-parent");
    let ws_dir = outer.path().join("ws");
    std::fs::create_dir_all(real_parent.join("sub")).unwrap();
    std::fs::create_dir_all(&ws_dir).unwrap();
    std::os::unix::fs::symlink(real_parent.join("sub"), ws_dir.join("link")).unwrap();

    let locks = Arc::new(PackOutputLocks::default());
    let first_guard = locks.lock(&real_parent.join("tarballs").join("pkg-a-1.0.0.tgz")).await;
    let second_acquired = Arc::new(AtomicBool::new(false));
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let waiter = {
        let locks = Arc::clone(&locks);
        let second_acquired = Arc::clone(&second_acquired);
        let symlink_path = ws_dir.join("link/../tarballs/pkg-a-1.0.0.tgz");
        tokio::spawn(async move {
            let _ = started_tx.send(());
            let _guard = locks.lock(&symlink_path).await;
            second_acquired.store(true, Ordering::SeqCst);
        })
    };

    started_rx.await.unwrap();
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(
        !second_acquired.load(Ordering::SeqCst),
        "second lock on equivalent symlink/../ path must wait while first guard is held",
    );

    drop(first_guard);
    waiter.await.unwrap();
    assert!(second_acquired.load(Ordering::SeqCst));
}
