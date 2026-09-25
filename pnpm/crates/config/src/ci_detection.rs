//! CI detection with the rules of the `ci-info` package, which pnpm 11
//! uses for its `ci` default. Keeping the two in step keeps frozen
//! installs and prompt suppression consistent across both CLIs.

use crate::api::{EnvVar, Host};
use std::sync::OnceLock;

/// Variables whose non-empty value marks a CI environment: `ci-info`'s
/// generic variables followed by the single-variable vendor checks from
/// its `vendors.json`.
pub const CI_ENV_VARS: &[&str] = &[
    "BUILD_ID",
    "BUILD_NUMBER",
    "CI",
    "CI_APP_ID",
    "CI_BUILD_ID",
    "CI_BUILD_NUMBER",
    "CI_NAME",
    "CONTINUOUS_INTEGRATION",
    "RUN_ID",
    "AGOLA_GIT_REF",
    "ALPIC_HOST",
    "AC_APPCIRCLE",
    "APPVEYOR",
    "CODEBUILD_BUILD_ARN",
    "TF_BUILD",
    "bamboo_planKey",
    "BITBUCKET_COMMIT",
    "BITRISE_IO",
    "BUDDY_WORKSPACE_ID",
    "BUILDKITE",
    "CIRCLECI",
    "CIRRUS_CI",
    "CF_PAGES",
    "WORKERS_CI",
    "CF_BUILD_ID",
    "CM_BUILD_ID",
    "DRONE",
    "DSARI",
    "EARTHLY_CI",
    "EAS_BUILD",
    "GERRIT_PROJECT",
    "GITEA_ACTIONS",
    "GITHUB_ACTIONS",
    "GITLAB_CI",
    "GO_PIPELINE_LABEL",
    "BUILDER_OUTPUT",
    "HARNESS_BUILD_ID",
    "HUDSON_URL",
    "LAYERCI",
    "MAGNUM",
    "NETLIFY",
    "NEVERCODE",
    "PROW_JOB_ID",
    "RELEASE_BUILD_ID",
    "RENDER",
    "SAILCI",
    "SCREWDRIVER",
    "SEMAPHORE",
    "STRIDER",
    "TEAMCITY_VERSION",
    "TRAVIS",
    "VELA",
    "NOW_BUILDER",
    "VERCEL",
    "APPCENTER_BUILD_ID",
    "CI_XCODE_PROJECT",
    "XCS",
];

const HEROKU_NODE_PATH: &str = "/app/.heroku/node/bin/node";

/// Whether the process runs in a CI environment. The environment is read
/// once per process.
pub fn is_ci() -> bool {
    static IS_CI: OnceLock<bool> = OnceLock::new();
    *IS_CI.get_or_init(detect_ci::<Host>)
}

/// Whether `Sys`'s environment is a CI environment. `CI=false` overrides
/// every other variable.
pub(crate) fn detect_ci<Sys: EnvVar>() -> bool {
    let is_set = |name: &str| Sys::var(name).is_some_and(|value| !value.is_empty());
    if Sys::var("CI").as_deref() == Some("false") {
        return false;
    }
    CI_ENV_VARS.iter().any(|name| is_set(name))
        || Sys::var("NODE").is_some_and(|node| node.contains(HEROKU_NODE_PATH))
}

#[cfg(test)]
mod tests;
