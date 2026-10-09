use assert_cmd::prelude::*;
use mockito::Matcher;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
    sync::{Arc, Mutex},
};

const ARTIFACT_PATH: &str = "^/v8/artifacts/[0-9a-f]+$";
const TEAM: &str = "team_pipeline";
const TOKEN: &str = "remote-cache-token";
const SIGNATURE_KEY: &str = "remote-cache-signature-key";

/// A one-project workspace whose `build` writes `out/result` and logs each
/// real run to `runs`, which lies outside the task's inputs and outputs.
fn workspace(root: &Path, server_url: &str) -> std::path::PathBuf {
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
        format!("packages: []\nincludeWorkspaceRoot: true\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n    inputs: ['src/**']\n    outputs: ['out/**']\npipelineRemoteCache:\n  url: {server_url}\n  team: {TEAM}\n"),
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

/// An uploaded artifact's body and its `x-artifact-tag`.
type UploadedArtifact = (Vec<u8>, String);

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn a_task_uploaded_from_one_machine_is_restored_on_another() {
    let mut server = mockito::Server::new();
    let root = tempfile::tempdir().unwrap();
    let project = workspace(root.path(), &server.url());
    let runs = root.path().join("runs");
    fs::write(&runs, "").unwrap();

    let builder = root.path().join("builder");
    fs::create_dir_all(builder.join("config/pnpm")).unwrap();
    fs::write(
        builder.join("config/pnpm/config.yaml"),
        format!("pipelineRemoteCache:\n  token: {TOKEN}\n  signatureKey: {SIGNATURE_KEY}\n  upload: true\n"),
    )
    .unwrap();
    pnpm_on_machine(&project, &builder, &runs)
        .arg("install")
        .assert()
        .success();

    let miss = server
        .mock("GET", Matcher::Regex(ARTIFACT_PATH.to_string()))
        .match_query(Matcher::UrlEncoded("teamId".to_string(), TEAM.to_string()))
        .match_header("authorization", format!("Bearer {TOKEN}").as_str())
        .with_status(404)
        .expect(1)
        .create();
    let uploaded: Arc<Mutex<Option<UploadedArtifact>>> = Arc::default();
    let upload = server
        .mock("PUT", Matcher::Regex(ARTIFACT_PATH.to_string()))
        .match_query(Matcher::UrlEncoded("teamId".to_string(), TEAM.to_string()))
        .match_header("authorization", format!("Bearer {TOKEN}").as_str())
        .match_header("x-artifact-duration", Matcher::Regex("^[0-9]+$".to_string()))
        .with_body_from_request({
            let uploaded = Arc::clone(&uploaded);
            move |request| {
                let tag = request.header("x-artifact-tag")[0]
                    .to_str()
                    .unwrap()
                    .to_string();
                *uploaded.lock().unwrap() = Some((request.body().unwrap().clone(), tag));
                Vec::new()
            }
        })
        .expect(1)
        .create();
    let built = pnpm_on_machine(&project, &builder, &runs)
        .args(["pipeline", "--full"])
        .assert()
        .success();
    miss.assert();
    upload.assert();
    assert!(
        !combined_output(built.get_output()).contains("restored from cache"),
        "{}",
        combined_output(built.get_output()),
    );
    assert_eq!(fs::read_to_string(&runs).unwrap(), "run\n");

    let (artifact, tag) = uploaded
        .lock()
        .unwrap()
        .take()
        .expect("the build uploaded an artifact");
    server.reset();
    let hit = server
        .mock("GET", Matcher::Regex(ARTIFACT_PATH.to_string()))
        .match_query(Matcher::UrlEncoded("teamId".to_string(), TEAM.to_string()))
        .with_header("x-artifact-tag", &tag)
        .with_body(&artifact)
        .create();
    fs::remove_dir_all(project.join("out")).unwrap();
    let consumer = root.path().join("consumer");
    let restored = pnpm_on_machine(&project, &consumer, &runs)
        .env("PNPM_PIPELINE_REMOTE_CACHE_TOKEN", TOKEN)
        .env("PNPM_PIPELINE_REMOTE_CACHE_SIGNATURE_KEY", SIGNATURE_KEY)
        .args(["pipeline", "--full"])
        .assert()
        .success();
    hit.assert();
    let output = combined_output(restored.get_output());
    assert!(output.contains("restored from cache"), "{output}");
    assert_eq!(fs::read_to_string(project.join("out/nested/result")).unwrap(), "built");
    assert_eq!(fs::read_to_string(&runs).unwrap(), "run\n", "the restored task must not run");

    fs::remove_dir_all(project.join("out")).unwrap();
    let untrusting = root.path().join("untrusting");
    let rejected = pnpm_on_machine(&project, &untrusting, &runs)
        .env("PNPM_PIPELINE_REMOTE_CACHE_TOKEN", TOKEN)
        .env("PNPM_PIPELINE_REMOTE_CACHE_SIGNATURE_KEY", "a-different-key")
        .args(["pipeline", "--full"])
        .assert()
        .success();
    let output = combined_output(rejected.get_output());
    assert!(output.contains("the artifact signature does not match"), "{output}");
    assert!(!output.contains("restored from cache"), "{output}");
    assert_eq!(fs::read_to_string(&runs).unwrap(), "run\nrun\n");
}

#[test]
fn a_remote_cache_without_a_signature_key_is_off() {
    let mut server = mockito::Server::new();
    let root = tempfile::tempdir().unwrap();
    let project = workspace(root.path(), &server.url());
    let runs = root.path().join("runs");
    fs::write(&runs, "").unwrap();
    let machine = root.path().join("machine");
    pnpm_on_machine(&project, &machine, &runs)
        .arg("install")
        .assert()
        .success();
    let any_request = server
        .mock("GET", Matcher::Any)
        .expect(0)
        .create();

    let result = pnpm_on_machine(&project, &machine, &runs)
        .env("PNPM_PIPELINE_REMOTE_CACHE_TOKEN", TOKEN)
        .args(["pipeline", "--full"])
        .assert()
        .success();

    any_request.assert();
    let output = combined_output(result.get_output());
    assert!(output.contains("pipelineRemoteCache.signatureKey is not set"), "{output}");
    assert_eq!(fs::read_to_string(&runs).unwrap(), "run\n");
}
