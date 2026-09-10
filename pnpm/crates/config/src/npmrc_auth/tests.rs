use std::path::Path;

use pnpm_network::{DEFAULT_REGISTRY_SCOPE, NoProxySetting};
use pretty_assertions::assert_eq;

use super::{
    BasicAuth, DeclaredRegistries, EnvVar, NpmrcAuth, RawCreds, RegistryCreds, base64_encode,
    credentials::base64_decode,
};
use crate::{Config, workspace_yaml::LoadWorkspaceYamlError};

/// Generate a per-test unit struct implementing [`EnvVar`] from a
/// `&[(&str, &str)]` literal — saves each cascade test from spelling
/// out an `impl EnvVar` block. Avoids touching the real process
/// environment so cascade tests don't need
/// [`pnpm_testing_utils::env_guard::EnvGuard`]'s global lock.
macro_rules! static_env {
    ($name:ident, $entries:expr) => {
        struct $name;
        impl EnvVar for $name {
            fn var(name: &str) -> Option<String> {
                let entries: &[(&str, &str)] = $entries;
                entries.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_string())
            }
        }
    };
}

/// Test fake: the process environment is empty. Per the DI
/// pattern from
/// [pnpm/pacquet#339](https://github.com/pnpm/pacquet/issues/339),
/// the fake is a unit struct scoped to the test module; tests
/// turbofish it through the generic slot.
struct NoEnv;
impl EnvVar for NoEnv {
    fn var(_: &str) -> Option<String> {
        None
    }
}

fn default_auth_token<'a>(auth: &'a NpmrcAuth, uri: &str) -> Option<Option<&'a str>> {
    auth.creds_by_scope_by_uri
        .get(uri)
        .and_then(|creds_by_scope| creds_by_scope.get(DEFAULT_REGISTRY_SCOPE))
        .map(|creds| creds.auth_token.as_deref())
}

fn scoped_auth_token<'a>(auth: &'a NpmrcAuth, uri: &str, scope: &str) -> Option<Option<&'a str>> {
    auth.creds_by_scope_by_uri
        .get(uri)
        .and_then(|creds_by_scope| creds_by_scope.get(scope))
        .map(|creds| creds.auth_token.as_deref())
}

// --- TLS + local-address tests ---

/// Same self-signed cert as `crates/network/src/tests.rs`; loaded from
/// the shared fixture under `crates/network/tests/fixtures/test-ca.pem`
/// so the base64 body stays out of the typos linter's dictionary.
const TEST_CA_PEM: &str = include_str!("../../../network/tests/fixtures/test-ca.pem");

/// Like [`static_env!`] but also overrides [`EnvVar::vars`] so the fake
/// can be enumerated — required by [`NpmrcAuth::from_url_scoped_env`],
/// which matches `npm_config_//…` / `pnpm_config_//…` vars by prefix.
macro_rules! static_env_with_vars {
    ($name:ident, $entries:expr) => {
        struct $name;
        impl EnvVar for $name {
            fn var(name: &str) -> Option<String> {
                let entries: &[(&str, &str)] = $entries;
                entries.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_string())
            }
            fn vars() -> Vec<(String, String)> {
                let entries: &[(&str, &str)] = $entries;
                entries.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
            }
        }
    };
}

mod behavior;

mod configuration;

mod authorization;

mod files;

mod reporting;

mod security;

mod streaming;
