use std::{
    fs,
    sync::Arc,
};

use pnpm_lockfile::{
    Lockfile,
    LockfileResolution,
};
use pnpm_resolving_resolver_base::{
    ResolutionVerification,
    ResolutionVerifier,
    VerifyCtx,
    VerifyFuture,
};
use tempfile::TempDir;

use super::record_lockfile_verified_by_content;
use crate::{
    CACHE_FILE_NAME,
    CacheRecord,
    ExoticSubdepsPolicy,
    hash_lockfile,
    try_lockfile_verification_cache,
};

const LOCKFILE: &str = "lockfileVersion: '9.0'

packages:

  react@17.0.2:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}
";

struct PassingVerifier {
    policy: serde_json::Map<String, serde_json::Value>,
}

impl ResolutionVerifier for PassingVerifier {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        Box::pin(async { ResolutionVerification::Ok })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

fn verifier() -> Arc<dyn ResolutionVerifier> {
    Arc::new(PassingVerifier { policy: serde_json::Map::new() })
}

fn parse_lockfile() -> Lockfile {
    serde_saphyr::from_str(LOCKFILE).expect("parse lockfile")
}

#[test]
fn records_the_hash_read_by_the_next_install() {
    let dir = TempDir::new().expect("tempdir");
    let first_path = dir.path().join("first/pnpm-lock.yaml");
    let second_path = dir.path().join("second/pnpm-lock.yaml");
    fs::create_dir_all(first_path.parent().expect("first parent")).expect("create first dir");
    fs::create_dir_all(second_path.parent().expect("second parent")).expect("create second dir");
    fs::write(&first_path, LOCKFILE).expect("write first lockfile");
    fs::write(&second_path, LOCKFILE).expect("write second lockfile");
    let written = parse_lockfile();
    let verifier = verifier();

    record_lockfile_verified_by_content(
        Some(dir.path()),
        &written,
        std::slice::from_ref(&verifier),
        &ExoticSubdepsPolicy::Allow,
    );
    let loaded: Lockfile =
        serde_saphyr::from_str(&fs::read_to_string(&second_path).expect("read lockfile"))
            .expect("load lockfile");
    let result = try_lockfile_verification_cache(
        dir.path(),
        &second_path,
        std::slice::from_ref(&verifier),
        || hash_lockfile(&loaded),
    );

    assert!(result.hit);
}

#[test]
fn records_content_only_post_save_and_caller_path_when_stat_supplied() {
    let dir = TempDir::new().expect("tempdir");
    let post_save_dir = dir.path().join("post-save");
    let with_stat_dir = dir.path().join("with-stat");
    let lockfile_path = dir.path().join("pnpm-lock.feature.yaml");
    fs::write(&lockfile_path, LOCKFILE).expect("write lockfile");
    let captured = crate::stat_from_metadata(&fs::metadata(&lockfile_path).expect("metadata"));
    let verifier = verifier();

    record_lockfile_verified_by_content(
        Some(&post_save_dir),
        &parse_lockfile(),
        std::slice::from_ref(&verifier),
        &ExoticSubdepsPolicy::Allow,
    );
    let post_save_cache =
        fs::read_to_string(post_save_dir.join(CACHE_FILE_NAME)).expect("read post-save cache");
    let post_save_record: CacheRecord =
        serde_json::from_str(post_save_cache.trim_end()).expect("parse post-save cache");
    assert!(
        post_save_record.lockfile.path.is_empty(),
        "post-save recorder must not attach unverified path metadata",
    );

    super::record_lockfile_verified_with_stat(
        Some(&with_stat_dir),
        &lockfile_path,
        captured,
        &parse_lockfile(),
        &[verifier],
        &ExoticSubdepsPolicy::Allow,
    );
    let stat_cache =
        fs::read_to_string(with_stat_dir.join(CACHE_FILE_NAME)).expect("read stat cache");
    let stat_record: CacheRecord =
        serde_json::from_str(stat_cache.trim_end()).expect("parse stat cache");
    assert_eq!(stat_record.lockfile.path, lockfile_path.to_string_lossy());
}

#[test]
fn refuses_to_record_a_lockfile_with_invalid_dependency_names() {
    let yaml = "lockfileVersion: '9.0'

packages:

  ../../../escape@1.0.0:
    resolution: {integrity: sha512-deadbeef}
";
    let dir = TempDir::new().expect("tempdir");
    let lockfile_path = dir.path().join("pnpm-lock.yaml");
    fs::write(&lockfile_path, yaml).expect("write lockfile");
    let lockfile: Lockfile = serde_saphyr::from_str(yaml).expect("parse lockfile");

    record_lockfile_verified_by_content(
        Some(dir.path()),
        &lockfile,
        &[verifier()],
        &ExoticSubdepsPolicy::Allow,
    );

    assert!(
        !dir.path()
            .join(CACHE_FILE_NAME)
            .exists(),
        "lockfile with invalid dependency names must never be recorded as verified",
    );
}

#[tokio::test]
async fn content_only_verification_caches_and_reuses_verdict_without_lockfile_path() {
    struct Silent;
    impl pnpm_reporter::Reporter for Silent {
        fn emit(_: &pnpm_reporter::LogEvent) {}
    }

    let dir = TempDir::new().expect("tempdir");
    let lockfile = parse_lockfile();
    let counting = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = Arc::clone(&counting);
    let verifier = Arc::new(CountingVerifier(calls))
        as Arc<dyn pnpm_resolving_resolver_base::ResolutionVerifier>;
    let opts = crate::VerifyLockfileResolutionsOptions {
        lockfile_path: None,
        cache_dir: Some(dir.path()),
        ..Default::default()
    };
    crate::verify_lockfile_resolutions::<Silent>(&lockfile, std::slice::from_ref(&verifier), &opts)
        .await
        .expect("first content-only verify");
    assert_eq!(counting.load(std::sync::atomic::Ordering::SeqCst), 1);

    crate::verify_lockfile_resolutions::<Silent>(&lockfile, std::slice::from_ref(&verifier), &opts)
        .await
        .expect("cached content-only verify");
    assert_eq!(counting.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(crate::lockfile_verification_is_cached_by_content(
        dir.path(),
        &lockfile,
        &[verifier],
        &ExoticSubdepsPolicy::Allow,
    ));
}

struct CountingVerifier(Arc<std::sync::atomic::AtomicUsize>);

impl pnpm_resolving_resolver_base::ResolutionVerifier for CountingVerifier {
    fn verify<'a>(
        &'a self,
        _resolution: &'a pnpm_lockfile::LockfileResolution,
        _ctx: pnpm_resolving_resolver_base::VerifyCtx<'a>,
    ) -> pnpm_resolving_resolver_base::VerifyFuture<'a> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { pnpm_resolving_resolver_base::ResolutionVerification::Ok })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        static EMPTY: std::sync::OnceLock<serde_json::Map<String, serde_json::Value>> =
            std::sync::OnceLock::new();
        EMPTY.get_or_init(serde_json::Map::new)
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

#[test]
fn pre_captured_stat_prevents_concurrent_file_swap_from_inheriting_verdict() {
    let dir = TempDir::new().expect("tempdir");
    let workspace = dir.path().join("ws");
    let cache_dir = dir.path().join("cache");
    fs::create_dir_all(&workspace).expect("create ws");
    fs::write(workspace.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").expect("write shared");
    let branch_path = workspace.join("pnpm-lock.feat.yaml");
    fs::write(&branch_path, LOCKFILE).expect("write branch lockfile");

    let selection = pnpm_lockfile::WantedLockfileSelection {
        branch_lockfile_candidates: vec!["pnpm-lock.feat.yaml".to_owned()],
        ..pnpm_lockfile::WantedLockfileSelection::default()
    };
    let lazy = pnpm_lockfile::LazyLockfile::deferred(workspace.clone(), selection);
    let verified = lazy
        .get()
        .expect("load")
        .expect("present")
        .clone();
    assert_eq!(lazy.loaded_path().expect("path"), Some(branch_path.as_path()));
    let captured = crate::stat_from_metadata(
        lazy.loaded_metadata()
            .expect("meta")
            .expect("stat"),
    );

    let tampered_yaml = "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      react:\n        specifier: 17.0.2\n        version: 17.0.2\npackages:\n  react@17.0.2:\n    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}\n  evil@https://example.com/evil.tgz:\n    resolution: {tarball: https://example.com/evil.tgz}\n    version: 1.0.0\nsnapshots:\n  react@17.0.2:\n    dependencies:\n      evil: https://example.com/evil.tgz\n  evil@https://example.com/evil.tgz: {}\n";
    let tmp_swap = workspace.join("pnpm-lock.feat.swap.yaml");
    fs::write(&tmp_swap, tampered_yaml).expect("write tampered");
    fs::rename(&tmp_swap, &branch_path).expect("atomic swap");

    let policy = ExoticSubdepsPolicy::block(crate::ExoticSubdepsScope::new(
        Some(["."]),
        std::iter::empty::<&str>(),
        std::iter::empty::<&str>(),
        std::iter::empty::<&str>(),
    ));
    super::record_lockfile_verified_with_stat(
        Some(&cache_dir),
        &branch_path,
        captured.clone(),
        &verified,
        &[verifier()],
        &policy,
    );

    let written_cache =
        fs::read_to_string(cache_dir.join(CACHE_FILE_NAME)).expect("read swap cache");
    let written_record: CacheRecord =
        serde_json::from_str(written_cache.trim_end()).expect("parse swap cache");
    assert_eq!(written_record.lockfile.hash, hash_lockfile(&verified));
    assert_eq!(written_record.lockfile.size, captured.size);
    assert_eq!(written_record.lockfile.mtime_ns, captured.mtime_ns);
    assert_eq!(written_record.lockfile.ctime_ns, captured.ctime_ns);
    assert_eq!(written_record.lockfile.inode, captured.inode);
    assert!(crate::lockfile_verification_is_cached_with_stat(
        &cache_dir,
        &branch_path,
        captured,
        &verified,
        &[verifier()],
        &policy,
    ));

    let reloaded = pnpm_lockfile::LazyLockfile::deferred(
        workspace,
        pnpm_lockfile::WantedLockfileSelection {
            branch_lockfile_candidates: vec!["pnpm-lock.feat.yaml".to_owned()],
            ..pnpm_lockfile::WantedLockfileSelection::default()
        },
    );
    let tampered = reloaded
        .get()
        .expect("reload")
        .expect("present");
    let tampered_stat = crate::stat_from_metadata(
        reloaded
            .loaded_metadata()
            .expect("meta")
            .expect("stat"),
    );
    assert!(
        !crate::lockfile_verification_is_cached_with_stat(
            &cache_dir,
            &branch_path,
            tampered_stat,
            tampered,
            &[verifier()],
            &policy,
        ),
        "swapped file must not inherit pre-swap verification verdict",
    );
}
