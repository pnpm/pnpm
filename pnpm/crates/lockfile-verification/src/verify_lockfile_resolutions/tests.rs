use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use pnpm_lockfile::{Lockfile, LockfileResolution, PkgName};
use pnpm_reporter::{LockfileVerificationMessage, LogEvent, Reporter, SilentReporter};
use pnpm_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};
use tempfile::TempDir;

use super::{
    VerifyLockfileResolutionsOptions, collect_resolution_policy_violations,
    verify_lockfile_resolutions,
};
use crate::VerifyError;

const SINGLE_PKG_LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      react:
        specifier: ^17.0.2
        version: 17.0.2

packages:

  react@17.0.2:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

snapshots:

  react@17.0.2: {}
";

const TWO_PKG_LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0
      bravo:
        specifier: ^2.0.0
        version: 2.0.0

packages:

  acme@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

  bravo@2.0.0:
    resolution: {integrity: sha512-s4h96KtLDUQlsENhMn1ar8t2bEa+q/YAtj8pPPdIjPDGBDIVNsrD9aXNWqspUe6AzKCIG0C1HZZLqLV7qpOBGA==}

snapshots:

  acme@1.0.0: {}
  bravo@2.0.0: {}
";

fn parse(yaml: &str) -> Lockfile {
    serde_saphyr::from_str(yaml).expect("parse fixture lockfile")
}

/// Reject every candidate with the given code/reason.
struct AlwaysFail {
    code: &'static str,
    reason: &'static str,
    policy: serde_json::Map<String, serde_json::Value>,
}

impl AlwaysFail {
    fn new(code: &'static str, reason: &'static str) -> Arc<Self> {
        Arc::new(Self { code, reason, policy: serde_json::Map::new() })
    }
}

impl ResolutionVerifier for AlwaysFail {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        let code = self.code;
        let reason = self.reason.to_string();
        Box::pin(async move { ResolutionVerification::Err { code, reason } })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

/// Reject only specific package names; pass everything else.
struct FailFor {
    code: &'static str,
    reason: &'static str,
    names: Vec<&'static str>,
    policy: serde_json::Map<String, serde_json::Value>,
}

impl FailFor {
    fn new(code: &'static str, reason: &'static str, names: Vec<&'static str>) -> Arc<Self> {
        Arc::new(Self { code, reason, names, policy: serde_json::Map::new() })
    }
}

impl ResolutionVerifier for FailFor {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        let name = ctx.name.to_string();
        let triggers = self.names.contains(&name.as_str());
        let code = self.code;
        let reason = self.reason.to_string();
        Box::pin(async move {
            if triggers {
                ResolutionVerification::Err { code, reason }
            } else {
                ResolutionVerification::Ok
            }
        })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

/// Abort every candidate with a transport failure (the registry couldn't be
/// reached to verify it).
struct FetchFails {
    message: &'static str,
    policy: serde_json::Map<String, serde_json::Value>,
}

impl FetchFails {
    fn new(message: &'static str) -> Arc<Self> {
        Arc::new(Self { message, policy: serde_json::Map::new() })
    }
}

impl ResolutionVerifier for FetchFails {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        let message = self.message.to_string();
        Box::pin(async move { ResolutionVerification::FetchFailed { message } })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

struct CapturingVerifier {
    seen: Arc<Mutex<Vec<LockfileResolution>>>,
    policy: serde_json::Map<String, serde_json::Value>,
}

impl ResolutionVerifier for CapturingVerifier {
    fn verify<'a>(
        &'a self,
        resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        self.seen.lock().expect("seen lock").push(resolution.clone());
        Box::pin(async { ResolutionVerification::Ok })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

mod behavior;

mod reporting;

mod lockfile;

mod streaming;

mod security;
