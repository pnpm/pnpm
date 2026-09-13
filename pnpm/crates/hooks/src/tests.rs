use super::{
    ChecksumFreeHooks, HookContext, HookError, PnpmfileHooks, PreResolutionHookContext,
    PreResolutionHookLogger, ReadPackageResult, current_pnpmfile_checksum,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

const CHECKSUM: &str = "sha256-inner";

struct CheckedHooks {
    source: PathBuf,
}

#[async_trait]
impl PnpmfileHooks for CheckedHooks {
    async fn read_package(
        &self,
        mut pkg: Value,
        _: HookContext,
    ) -> Result<ReadPackageResult, HookError> {
        pkg["hooked"] = json!(true);
        Ok(Arc::new(pkg))
    }

    async fn after_all_resolved(&self, _: Value, _: HookContext) -> Result<Value, HookError> {
        Ok(Value::Null)
    }

    async fn pre_resolution(&self, _: PreResolutionHookContext, _: PreResolutionHookLogger) {}

    async fn filter_log(&self, _: Value, _: HookContext) -> bool {
        true
    }

    async fn calculate_pnpmfile_checksum(&self) -> Option<String> {
        Some(CHECKSUM.to_string())
    }

    fn source_path(&self) -> Option<&Path> {
        Some(&self.source)
    }
}

fn wrapped_at(source: PathBuf) -> Arc<dyn PnpmfileHooks> {
    let inner: Arc<dyn PnpmfileHooks> = Arc::new(CheckedHooks { source });
    Arc::new(ChecksumFreeHooks::from(inner))
}

fn wrapped() -> Arc<dyn PnpmfileHooks> {
    wrapped_at(PathBuf::from("/workspace/.pnpmfile.mjs"))
}

#[tokio::test]
async fn checksum_free_hooks_still_run_their_hooks() {
    let hooked = wrapped()
        .read_package(json!({ "name": "app" }), HookContext { log: Arc::new(|_| {}), dir: None })
        .await
        .expect("run the wrapped readPackage hook");

    assert_eq!(hooked["hooked"], json!(true));
}

#[tokio::test]
async fn checksum_free_hooks_contribute_no_checksum() {
    assert_eq!(wrapped().calculate_pnpmfile_checksum().await, None);
}

/// Pins the bound on [`ChecksumFreeHooks`]: it suppresses the checksum
/// only for a lockfile that records none, because the wrapper keeps
/// answering `source_path` and [`current_pnpmfile_checksum`] hashes that
/// file whenever there is a recorded value to compare against.
#[tokio::test]
async fn a_recorded_checksum_is_still_answered_from_the_pnpmfile() {
    let dir = tempfile::tempdir().expect("create a temp dir");
    let pnpmfile = dir.path().join(".pnpmfile.mjs");
    std::fs::write(&pnpmfile, "export const hooks = {}\n").expect("write a pnpmfile");
    let hooks = wrapped_at(pnpmfile.clone());

    assert_eq!(current_pnpmfile_checksum(Some(&hooks), None).await, None);

    let against_recorded = current_pnpmfile_checksum(Some(&hooks), Some("sha256-recorded")).await;
    assert_eq!(
        against_recorded,
        pnpm_crypto_hash::create_hash_from_file(&pnpmfile).ok(),
        "a recorded checksum is compared against the file `source_path` names",
    );
}
