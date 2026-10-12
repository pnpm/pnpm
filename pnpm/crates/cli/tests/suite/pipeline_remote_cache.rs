use assert_cmd::prelude::*;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use p256::{
    SecretKey,
    pkcs8::{EncodePrivateKey as _, EncodePublicKey as _},
};
use pnpm_testing_utils::{command_env::CommandTestExt, turborepo_cache::TurborepoCache};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const TEAM: &str = "team_pipeline";
const TOKEN: &str = "remote-cache-token";
const KEY_ID: &str = "ci-2026";

/// The base64 private and public halves of a fixture key pair.
fn key_pair(seed: u8) -> (String, String) {
    let secret = SecretKey::from_slice(&[seed; 32]).expect("fixture private key");
    let private_key = BASE64.encode(
        secret
            .to_pkcs8_der()
            .expect("encode private key")
            .as_bytes(),
    );
    let public_key = BASE64.encode(
        secret
            .public_key()
            .to_public_key_der()
            .expect("encode public key")
            .as_bytes(),
    );
    (private_key, public_key)
}

/// A one-project workspace whose `build` writes `out/nested/result` and logs
/// each real run to `runs`, which lies outside the task's inputs and outputs.
fn workspace(root: &Path) -> PathBuf {
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    pnpm_testing_utils::git_repo::init_isolated_repo(&project);
    fs::write(project.join(".gitignore"), "out/\nnode_modules/\n").unwrap();
    fs::create_dir(project.join("src")).unwrap();
    fs::write(project.join("src/input"), "source").unwrap();
    fs::write(project.join("package.json"), serde_json::json!({
        "name": "probe", "version": "1.0.0", "scripts": {
            "build": r#"node -e "const fs=require('fs');fs.mkdirSync('out/nested',{recursive:true});fs.writeFileSync('out/nested/result','built');fs.appendFileSync(process.env.RUN_LOG,'run\n')""#
        }
    }).to_string()).unwrap();
    fs::write(
        project.join("pnpm-workspace.yaml"),
        "packages: []\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n    inputs: ['src/**']\n    outputs: ['out/**']\nremoteCache:\n  org: acme\n",
    )
    .unwrap();
    project
}

/// `pnpm` in `project` with a local cache and global config of its own under
/// `machine`, as a separate machine would have.
fn pnpm_on_machine(project: &Path, machine: &Path, runs: &Path) -> Command {
    let mut command = Command::cargo_bin("pnpm").unwrap().without_ambient_pnpm_config();
    command
        .current_dir(project)
        .env("XDG_CACHE_HOME", machine.join("cache"))
        .env("XDG_CONFIG_HOME", machine.join("config"))
        .env("RUN_LOG", runs);
    command
}

/// The auth setting that holds the token for `cache`, as a machine's auth
/// file or environment names it.
fn cache_token_key(cache: &TurborepoCache) -> String {
    format!("{}:_authToken", pnpm_network::nerf_dart(&format!("{}/", cache.url())))
}

/// [`pnpm_on_machine`] on a machine that names `cache` in its environment.
fn pnpm_with_cache(project: &Path, machine: &Path, runs: &Path, cache: &TurborepoCache) -> Command {
    let mut command = pnpm_on_machine(project, machine, runs);
    command
        .env("PNPM_REMOTE_CACHE_URL", cache.url())
        .env("PNPM_REMOTE_CACHE_TEAM", TEAM)
        .env(format!("pnpm_config_{}", cache_token_key(cache)), TOKEN);
    command
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn a_task_published_from_one_machine_is_restored_on_another() {
    let cache = TurborepoCache::start();
    let root = tempfile::tempdir().unwrap();
    let project = workspace(root.path());
    let runs = root.path().join("runs");
    fs::write(&runs, "").unwrap();
    let (private_key, public_key) = key_pair(7);

    let builder = root.path().join("builder");
    fs::create_dir_all(builder.join("config/pnpm")).unwrap();
    fs::write(
        builder.join("config/pnpm/config.yaml"),
        format!("remoteCache:\n  url: {}\n  team: {TEAM}\n  trustedKeys:\n    {KEY_ID}: {public_key}\n  privateKey: {private_key}\n  keyId: {KEY_ID}\n  builderId: ci/main/1\n  publish: true\n", cache.url()),
    )
    .unwrap();
    fs::write(
        builder.join("config/pnpm/auth.ini"),
        format!("{}={TOKEN}\n", cache_token_key(&cache)),
    )
    .unwrap();
    pnpm_on_machine(&project, &builder, &runs)
        .arg("install")
        .assert()
        .success();
    let built = pnpm_on_machine(&project, &builder, &runs)
        .args(["pipeline", "--full"])
        .assert()
        .success();
    let output = combined_output(built.get_output());
    assert!(!output.contains("restored from cache"), "{output}");
    assert_eq!(fs::read_to_string(&runs).unwrap(), "run\n");
    assert!(!cache.artifacts().is_empty(), "{output}");
    let requests = cache.requests();
    dbg!(&requests);
    assert!(
        requests
            .iter()
            .all(|request| {
                request.query.as_deref() == Some("teamId=team_pipeline")
                    && request.authorization.as_deref() == Some(&format!("Bearer {TOKEN}"))
            }),
    );

    fs::remove_dir_all(project.join("out")).unwrap();
    let consumer = root.path().join("consumer");
    let restored = pnpm_with_cache(&project, &consumer, &runs, &cache)
        .env("PNPM_REMOTE_CACHE_TRUSTED_KEYS", format!(r#"{{"{KEY_ID}":"{public_key}"}}"#))
        .args(["pipeline", "--full"])
        .assert()
        .success();
    let output = combined_output(restored.get_output());
    assert!(output.contains("restored from cache"), "{output}");
    assert_eq!(fs::read_to_string(project.join("out/nested/result")).unwrap(), "built");
    assert_eq!(fs::read_to_string(&runs).unwrap(), "run\n", "the restored task must not run");

    fs::remove_dir_all(project.join("out")).unwrap();
    let (_, other_public_key) = key_pair(9);
    let untrusting = root.path().join("untrusting");
    let rejected = pnpm_with_cache(&project, &untrusting, &runs, &cache)
        .env("PNPM_REMOTE_CACHE_TRUSTED_KEYS", format!(r#"{{"{KEY_ID}":"{other_public_key}"}}"#))
        .args(["pipeline", "--full"])
        .assert()
        .success();
    let output = combined_output(rejected.get_output());
    assert!(!output.contains("restored from cache"), "{output}");
    assert_eq!(fs::read_to_string(&runs).unwrap(), "run\nrun\n");
}

#[test]
fn a_remote_cache_without_trusted_keys_is_off() {
    let cache = TurborepoCache::start();
    let root = tempfile::tempdir().unwrap();
    let project = workspace(root.path());
    let runs = root.path().join("runs");
    fs::write(&runs, "").unwrap();
    let machine = root.path().join("machine");
    pnpm_on_machine(&project, &machine, &runs)
        .arg("install")
        .assert()
        .success();

    let result = pnpm_with_cache(&project, &machine, &runs, &cache)
        .args(["pipeline", "--full"])
        .assert()
        .success();

    assert!(cache.requests().is_empty());
    let output = combined_output(result.get_output());
    assert!(output.contains("remoteCache.trustedKeys is not set"), "{output}");
    assert_eq!(fs::read_to_string(&runs).unwrap(), "run\n");
}

#[test]
fn a_run_report_needs_the_organization_to_record_it_under() {
    let root = tempfile::tempdir().unwrap();
    let project = workspace(root.path());
    fs::write(
        project.join("pnpm-workspace.yaml"),
        "packages: []\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n",
    )
    .unwrap();
    let runs = root.path().join("runs");
    fs::write(&runs, "").unwrap();
    let machine = root.path().join("machine");
    pnpm_on_machine(&project, &machine, &runs)
        .arg("install")
        .assert()
        .success();

    let result = pnpm_on_machine(&project, &machine, &runs)
        .args(["pipeline", "--full", "--report-to", "http://127.0.0.1:9"])
        .assert()
        .success();

    let output = combined_output(result.get_output());
    assert!(output.contains("remoteCache.org does not name the organization"), "{output}");
}

/// A machine that publishes dependency builds has not agreed to share task
/// outputs and their logs: only `remoteCache.publish` publishes tasks.
#[test]
fn publishing_dependency_builds_does_not_publish_tasks() {
    let cache = TurborepoCache::start();
    let root = tempfile::tempdir().unwrap();
    let project = workspace(root.path());
    let runs = root.path().join("runs");
    fs::write(&runs, "").unwrap();
    let (private_key, public_key) = key_pair(7);
    let builder = root.path().join("builder");
    fs::create_dir_all(builder.join("config/pnpm")).unwrap();
    fs::write(
        builder.join("config/pnpm/config.yaml"),
        format!("sideEffectsCache:\n  remote:\n    trustedKeys:\n      {KEY_ID}: {public_key}\n    privateKey: {private_key}\n    keyId: {KEY_ID}\n    builderId: ci/main/1\n    publish: true\n"),
    )
    .unwrap();
    pnpm_on_machine(&project, &builder, &runs)
        .arg("install")
        .assert()
        .success();

    pnpm_with_cache(&project, &builder, &runs, &cache)
        .args(["pipeline", "--full"])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&runs).unwrap(), "run\n");
    assert!(cache.artifacts().is_empty(), "no task may be published");
    assert!(!cache.requests().is_empty(), "the remote cache is still consulted");
}
