//! The GitLab CI variables that npm puts in the SLSA v0.2 predicate's
//! `invocation.parameters`. The npm registry checks some of them against the
//! signing certificate. The list mirrors
//! <https://github.com/npm/cli/blob/b317f16c80df02ea3628cfa77170d5ae9b59720c/workspaces/libnpmpublish/lib/provenance.js#L83-L161>.

use pipe_trait::Pipe;
use serde_json::{Map, Value};

use crate::capabilities::EnvVar;

const GITLAB_PARAMETER_NAMES: &[&str] = &[
    "CI",
    "CI_API_GRAPHQL_URL",
    "CI_API_V4_URL",
    "CI_BUILD_BEFORE_SHA",
    "CI_BUILD_ID",
    "CI_BUILD_NAME",
    "CI_BUILD_REF",
    "CI_BUILD_REF_NAME",
    "CI_BUILD_REF_SLUG",
    "CI_BUILD_STAGE",
    "CI_COMMIT_BEFORE_SHA",
    "CI_COMMIT_BRANCH",
    "CI_COMMIT_REF_NAME",
    "CI_COMMIT_REF_PROTECTED",
    "CI_COMMIT_REF_SLUG",
    "CI_COMMIT_SHA",
    "CI_COMMIT_SHORT_SHA",
    "CI_COMMIT_TIMESTAMP",
    "CI_COMMIT_TITLE",
    "CI_CONFIG_PATH",
    "CI_DEFAULT_BRANCH",
    "CI_DEPENDENCY_PROXY_DIRECT_GROUP_IMAGE_PREFIX",
    "CI_DEPENDENCY_PROXY_GROUP_IMAGE_PREFIX",
    "CI_DEPENDENCY_PROXY_SERVER",
    "CI_DEPENDENCY_PROXY_USER",
    "CI_JOB_ID",
    "CI_JOB_NAME",
    "CI_JOB_NAME_SLUG",
    "CI_JOB_STAGE",
    "CI_JOB_STARTED_AT",
    "CI_JOB_URL",
    "CI_NODE_TOTAL",
    "CI_PAGES_DOMAIN",
    "CI_PAGES_URL",
    "CI_PIPELINE_CREATED_AT",
    "CI_PIPELINE_ID",
    "CI_PIPELINE_IID",
    "CI_PIPELINE_SOURCE",
    "CI_PIPELINE_URL",
    "CI_PROJECT_CLASSIFICATION_LABEL",
    "CI_PROJECT_DESCRIPTION",
    "CI_PROJECT_ID",
    "CI_PROJECT_NAME",
    "CI_PROJECT_NAMESPACE",
    "CI_PROJECT_NAMESPACE_ID",
    "CI_PROJECT_PATH",
    "CI_PROJECT_PATH_SLUG",
    "CI_PROJECT_REPOSITORY_LANGUAGES",
    "CI_PROJECT_ROOT_NAMESPACE",
    "CI_PROJECT_TITLE",
    "CI_PROJECT_URL",
    "CI_PROJECT_VISIBILITY",
    "CI_REGISTRY",
    "CI_REGISTRY_IMAGE",
    "CI_REGISTRY_USER",
    "CI_RUNNER_DESCRIPTION",
    "CI_RUNNER_ID",
    "CI_RUNNER_TAGS",
    "CI_SERVER_HOST",
    "CI_SERVER_NAME",
    "CI_SERVER_PORT",
    "CI_SERVER_PROTOCOL",
    "CI_SERVER_REVISION",
    "CI_SERVER_SHELL_SSH_HOST",
    "CI_SERVER_SHELL_SSH_PORT",
    "CI_SERVER_URL",
    "CI_SERVER_VERSION",
    "CI_SERVER_VERSION_MAJOR",
    "CI_SERVER_VERSION_MINOR",
    "CI_SERVER_VERSION_PATCH",
    "CI_TEMPLATE_REGISTRY_HOST",
    "GITLAB_CI",
    "GITLAB_FEATURES",
    "GITLAB_USER_ID",
    "GITLAB_USER_LOGIN",
    "RUNNER_GENERATE_ARTIFACTS_METADATA",
];

/// Every variable in [`GITLAB_PARAMETER_NAMES`] that is set, keyed by its
/// name. An unset variable is left out, as npm leaves it out.
pub(super) fn gitlab_parameters<Sys: EnvVar>() -> Value {
    GITLAB_PARAMETER_NAMES
        .iter()
        .filter_map(|&name| Sys::var(name).map(|value| (name.to_owned(), Value::String(value))))
        .collect::<Map<_, _>>()
        .pipe(Value::Object)
}
