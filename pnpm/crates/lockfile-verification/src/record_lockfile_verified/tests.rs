use std::{fs, sync::Arc};

use pacquet_lockfile::{Lockfile, LockfileResolution};
use pacquet_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};
use tempfile::TempDir;

use super::record_lockfile_verified;
use crate::{CACHE_FILE_NAME, CacheRecord, hash_lockfile, try_lockfile_verification_cache};

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
