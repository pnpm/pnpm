use std::{fs, sync::Arc};

use pnpm_lockfile::{Lockfile, LockfileResolution};
use pnpm_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};
use tempfile::TempDir;

use super::{pending_lockfile_verified, record_lockfile_verified};
use crate::{
    CACHE_FILE_NAME, CacheRecord, hash_lockfile, persist_record, try_lockfile_verification_cache,
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

    record_lockfile_verified(
        Some(dir.path()),
        &first_path,
        &written,
        std::slice::from_ref(&verifier),
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
fn records_the_caller_supplied_lockfile_path() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile_path = dir.path().join("pnpm-lock.feature.yaml");
    fs::write(&lockfile_path, LOCKFILE).expect("write lockfile");
    let verifier = verifier();

    record_lockfile_verified(Some(dir.path()), &lockfile_path, &parse_lockfile(), &[verifier]);

    let cache = fs::read_to_string(dir.path().join(CACHE_FILE_NAME)).expect("read cache");
    let record: CacheRecord = serde_json::from_str(cache.trim_end()).expect("parse cache");
    assert_eq!(record.lockfile.path, lockfile_path.to_string_lossy());
}

fn cache_lines(cache_dir: &std::path::Path) -> usize {
    fs::read_to_string(cache_dir.join(CACHE_FILE_NAME))
        .map_or(0, |contents| {
            contents
                .lines()
                .filter(|line| !line.is_empty())
                .count()
        })
}

#[test]
fn a_content_hash_hit_hands_back_its_path_refresh_unwritten() {
    let dir = TempDir::new().expect("tempdir");
    let first_path = dir.path().join("first/pnpm-lock.yaml");
    let second_path = dir.path().join("second/pnpm-lock.yaml");
    for path in [&first_path, &second_path] {
        fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        fs::write(path, LOCKFILE).expect("write lockfile");
    }
    let lockfile = parse_lockfile();
    let verifier = verifier();
    record_lockfile_verified(
        Some(dir.path()),
        &first_path,
        &lockfile,
        std::slice::from_ref(&verifier),
    );

    let result = try_lockfile_verification_cache(
        dir.path(),
        &second_path,
        std::slice::from_ref(&verifier),
        || hash_lockfile(&lockfile),
    );
    assert!(result.hit);
    assert_eq!(cache_lines(dir.path()), 1, "the lookup must not write the refresh");

    persist_record(dir.path(), &result.refresh.expect("a content hit carries a refresh"));
    let stat_only = try_lockfile_verification_cache(
        dir.path(),
        &second_path,
        std::slice::from_ref(&verifier),
        || panic!("the persisted refresh must let the stat shortcut answer"),
    );
    assert!(stat_only.hit);
}

#[test]
fn a_pending_record_keys_the_lockfile_as_it_was_when_verified() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile_path = dir.path().join("pnpm-lock.yaml");
    fs::write(&lockfile_path, LOCKFILE).expect("write lockfile");
    let verified = parse_lockfile();
    let verifier = verifier();

    let pending = pending_lockfile_verified(
        Some(dir.path()),
        &lockfile_path,
        &verified,
        std::slice::from_ref(&verifier),
    )
    .expect("a cache dir and verifiers yield a pending record");
    // A lifecycle script rewrites the lockfile before the install records.
    let rewritten_text = LOCKFILE.replace("react@17.0.2", "react@17.0.10");
    fs::write(&lockfile_path, &rewritten_text).expect("rewrite lockfile");
    pending.record();

    let rewritten: Lockfile = serde_saphyr::from_str(&rewritten_text).expect("parse rewritten");
    let result = try_lockfile_verification_cache(
        dir.path(),
        &lockfile_path,
        std::slice::from_ref(&verifier),
        || hash_lockfile(&rewritten),
    );
    assert!(!result.hit, "the rewritten lockfile must not inherit the verdict");
}
