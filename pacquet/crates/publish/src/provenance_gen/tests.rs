use super::{github_statement, npm_purl};
use crate::capabilities::EnvVar;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn purl_encodes_only_the_leading_scope_at() {
    assert_eq!(npm_purl("pkg", "1.0.0"), "pkg:npm/pkg@1.0.0");
    assert_eq!(npm_purl("@scope/pkg", "1.2.3"), "pkg:npm/%40scope/pkg@1.2.3");
}

/// A fake environment that mirrors the variables GitHub Actions sets, so the
/// statement builder can be exercised without a real CI runner.
struct GhEnv;

impl EnvVar for GhEnv {
    fn var(name: &str) -> Option<String> {
        let value = match name {
            "GITHUB_SERVER_URL" => "https://github.com",
            "GITHUB_REPOSITORY" => "pnpm/pnpm",
            "GITHUB_REPOSITORY_ID" => "123",
            "GITHUB_REPOSITORY_OWNER_ID" => "456",
            "GITHUB_WORKFLOW_REF" => "pnpm/pnpm/.github/workflows/release.yml@refs/heads/main",
            "GITHUB_EVENT_NAME" => "push",
            "GITHUB_REF" => "refs/heads/main",
            "GITHUB_SHA" => "abc123",
            "GITHUB_RUN_ID" => "42",
            "GITHUB_RUN_ATTEMPT" => "1",
            "RUNNER_ENVIRONMENT" => "github-hosted",
            _ => return None,
        };
        Some(value.to_owned())
    }
}

#[test]
fn github_statement_shapes_the_slsa_v1_predicate() {
    let subject = json!([{ "name": "pkg:npm/pkg@1.0.0", "digest": { "sha512": "deadbeef" } }]);
    let statement = github_statement::<GhEnv>(&subject);

    assert_eq!(statement["_type"], "https://in-toto.io/Statement/v1");
    assert_eq!(statement["predicateType"], "https://slsa.dev/provenance/v1");
    let workflow = &statement["predicate"]["buildDefinition"]["externalParameters"]["workflow"];
    // GITHUB_WORKFLOW_REF has the `owner/repo/` prefix stripped, then splits on `@`.
    assert_eq!(workflow["path"], ".github/workflows/release.yml");
    assert_eq!(workflow["ref"], "refs/heads/main");
    assert_eq!(workflow["repository"], "https://github.com/pnpm/pnpm");
    let resolved = &statement["predicate"]["buildDefinition"]["resolvedDependencies"][0];
    assert_eq!(resolved["uri"], "git+https://github.com/pnpm/pnpm@refs/heads/main");
    assert_eq!(resolved["digest"]["gitCommit"], "abc123");
    let run = &statement["predicate"]["runDetails"];
    assert_eq!(run["builder"]["id"], "https://github.com/actions/runner/github-hosted");
    assert_eq!(
        run["metadata"]["invocationId"],
        "https://github.com/pnpm/pnpm/actions/runs/42/attempts/1",
    );
}
