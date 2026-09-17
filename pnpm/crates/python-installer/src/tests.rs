use super::environment::{accept_server_lockfile, resolve_via_pnpr};
use pnpm_config::Config;
use pnpm_python_resolver::Target;
use std::path::Path;

fn target() -> Target {
    let environment = serde_json::from_value(serde_json::json!({
        "implementation_name": "cpython",
        "implementation_version": "3.12.0",
        "os_name": "posix",
        "platform_machine": "x86_64",
        "platform_release": "6.1.0",
        "platform_system": "Linux",
        "platform_version": "#1 SMP",
        "python_full_version": "3.12.0",
        "platform_python_implementation": "CPython",
        "python_version": "3.12",
        "sys_platform": "linux",
    }))
    .expect("marker environment fixture");
    Target { environment, tags: vec!["py3-none-any".to_string()] }
}

fn handshake_body(ecosystems: &[&str]) -> String {
    serde_json::json!({
        "pnpr": { "versions": [0], "artifacts": [], "fixLockfile": [0], "ecosystems": ecosystems },
    })
    .to_string()
}

fn config_for_pnpr(server: &str) -> Config {
    let mut config = Config::new();
    config.pnpr_server = Some(server.to_string());
    config
}

fn requirements(specifiers: &[&str]) -> Vec<pep508_rs::Requirement> {
    specifiers
        .iter()
        .map(|requirement| pnpm_python_resolver::parse_requirement(requirement))
        .collect::<miette::Result<Vec<_>>>()
        .expect("requirement fixtures")
}

fn server_lockfile(index: &str) -> serde_json::Value {
    serde_json::json!({
        "lock-version": "1.0",
        "created-by": "pnpm",
        "environments": ["sys_platform == 'linux'"],
        "packages": [{
            "name": "demo",
            "version": "1.0.0",
            "wheels": [{
                "name": "demo-1.0.0-py3-none-any.whl",
                "url": format!("{index}demo-1.0.0-py3-none-any.whl"),
                "hashes": { "sha256": "a".repeat(64) },
            }],
        }],
        "tool": { "pnpm": {
            "requirements": ["demo"],
            "environment": target().environment,
            "tags": target().tags,
            "index": index,
        } },
    })
}

#[tokio::test]
async fn python_resolution_is_offloaded_to_the_pnpr_server() {
    let mut server = mockito::Server::new_async().await;
    let index = "https://index.example.test/simple/";
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "pypi"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "ecosystem": "pypi",
            "requirements": ["demo"],
            "index": index,
        })))
        .with_header("content-type", "application/x-ndjson")
        .with_body(format!(
            "{}\n",
            serde_json::json!({ "type": "done", "lockfile": server_lockfile(index) }),
        ))
        .expect(1)
        .create_async()
        .await;

    let resolved = resolve_via_pnpr(
        &config_for_pnpr(&server.url()),
        &requirements(&["demo"]),
        &target(),
        index,
        None,
    )
    .await
    .unwrap()
    .expect("the server resolves Python");

    assert_eq!(resolved.packages[0].name.as_ref(), "demo");
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn a_server_without_python_support_leaves_resolution_local() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .expect(0)
        .create_async()
        .await;

    let resolved = resolve_via_pnpr(
        &config_for_pnpr(&server.url()),
        &requirements(&["demo"]),
        &target(),
        "https://index.example.test/simple/",
        None,
    )
    .await
    .unwrap();

    assert!(resolved.is_none());
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn an_offline_install_does_not_reach_the_pnpr_server() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .expect(0)
        .create_async()
        .await;
    let mut config = config_for_pnpr(&server.url());
    config.offline = true;

    let resolved = resolve_via_pnpr(
        &config,
        &requirements(&["demo"]),
        &target(),
        "https://index.example.test/simple/",
        None,
    )
    .await
    .unwrap();

    assert!(resolved.is_none());
    handshake.assert_async().await;
}

#[test]
fn a_lockfile_answering_another_question_is_refused() {
    let index = "https://index.example.test/simple/";
    let inputs = pnpm_python_resolver::Inputs::new(&requirements(&["demo"]), &target(), index);
    let answered: pnpm_python_resolver::Lockfile =
        serde_json::from_value(server_lockfile(index)).expect("lockfile fixture");

    accept_server_lockfile(&answered, &inputs, None).expect("the same question");

    let other_requirements =
        pnpm_python_resolver::Inputs::new(&requirements(&["demo", "extra"]), &target(), index);
    let error = accept_server_lockfile(&answered, &other_requirements, None)
        .expect_err("other requirements");
    assert!(error.to_string().contains("for other inputs"), "{error}");
    let error = accept_server_lockfile(&answered, &inputs, Some(">=3.12"))
        .expect_err("another requires-python");
    assert!(error.to_string().contains("for other inputs"), "{error}");
}

/// A server answers for the requirements alone, so the members that asked
/// them together are the install's to record before its answer is held
/// against the inputs.
#[test]
fn a_server_lockfile_is_accepted_once_the_members_are_recorded() {
    let index = "https://index.example.test/simple/";
    let mut inputs = pnpm_python_resolver::Inputs::new(&requirements(&["demo"]), &target(), index);
    inputs.set_members(vec!["packages/a".to_string(), "packages/b".to_string()]);
    let mut answered: pnpm_python_resolver::Lockfile =
        serde_json::from_value(server_lockfile(index)).expect("lockfile fixture");

    let error = accept_server_lockfile(&answered, &inputs, None).expect_err("no members yet");
    assert!(error.to_string().contains("for other inputs"), "{error}");
    answered.tool.pnpm.set_members(inputs.members().to_vec());
    accept_server_lockfile(&answered, &inputs, None).expect("the same question");
}

/// A lockfile records a workspace project by a path that still means the
/// same project in another checkout of the repository.
#[test]
fn a_workspace_project_is_recorded_relative_to_the_lockfile() {
    let relative =
        |from: &str, to: &str| super::workspace::relative(Path::new(from), Path::new(to));
    assert_eq!(relative("/repo/packages/app", "/repo/packages/lib"), "../lib");
    assert_eq!(relative("/repo", "/repo/packages/lib"), "packages/lib");
    assert_eq!(relative("/repo/app", "/repo/app"), ".");
    assert_eq!(relative("/repo/a/b/c", "/repo/lib"), "../../../lib");
}

fn manifest(contents: &str) -> std::sync::Arc<super::manifest::Manifest> {
    std::sync::Arc::new(super::manifest::Manifest::parse(contents).expect("manifest fixture"))
}

fn workspace_of(
    projects: &[(&str, &str)],
) -> miette::Result<(super::workspace::Workspace, Vec<std::path::PathBuf>)> {
    let projects = projects
        .iter()
        .map(|(root, contents)| (std::path::PathBuf::from(root), manifest(contents)))
        .collect::<Vec<_>>();
    let roots = projects
        .iter()
        .map(|(root, _)| root.clone())
        .collect();
    Ok((super::workspace::Workspace::new(&projects)?, roots))
}

const SHARED_ROOT: &str = "[tool.uv.workspace]\nmembers = ['packages/*']\nexclude = ['packages/tool']\n\n[tool.pnpm.python]\nshared-environment = true\n";
const MEMBER: &str = "[project]\nname = 'member'\nversion = '1.0'\n";

/// The members of a workspace whose root asks for a shared environment
/// install into the root; a project the declaration excludes, and one
/// outside it, keep their own.
#[test]
fn a_shared_workspace_groups_its_members_and_leaves_the_others_alone() {
    let (workspace, roots) = workspace_of(&[
        ("/repo", SHARED_ROOT),
        ("/repo/packages/a", MEMBER),
        ("/repo/packages/b", MEMBER),
        ("/repo/packages/tool", MEMBER),
        ("/repo/vendor/c", MEMBER),
    ])
    .expect("a shared workspace");
    let selected = roots[1..].iter().cloned().collect();

    let memberships = workspace.memberships(&selected);
    let grouped = memberships
        .iter()
        .map(|membership| {
            (membership.root.display().to_string(), membership.shared, membership.members.len())
        })
        .collect::<Vec<_>>();
    dbg!(&grouped);
    assert_eq!(
        grouped,
        [
            ("/repo".to_string(), true, 2),
            ("/repo/packages/tool".to_string(), false, 1),
            ("/repo/vendor/c".to_string(), false, 1),
        ],
    );
    assert_eq!(workspace.lock_root(&roots[1]), std::path::Path::new("/repo"));
    assert_eq!(workspace.lock_root(&roots[3]), roots[3].as_path());
    // A root without a project of its own is nobody's member, and a root
    // that declares one is its own.
    assert_eq!(memberships[0].members, roots[1..3]);
}

#[test]
fn a_shared_environment_needs_a_workspace_to_share_it() {
    let error = workspace_of(&[(
        "/repo",
        "[project]\nname = 'app'\nversion = '1.0'\n\n[tool.pnpm.python]\nshared-environment = true\n",
    )])
    .err()
    .expect("refused");
    assert!(error.to_string().contains("declares no [tool.uv.workspace]"), "{error}");
}

#[test]
fn the_members_of_a_shared_environment_share_the_interpreter_range_all_accept() {
    let members = [
        ("/repo/a", manifest("[project]\nname = 'a'\nrequires-python = '>=3.10'\n")),
        ("/repo/b", manifest("[project]\nname = 'b'\nrequires-python = '>=3.10, !=3.15'\n")),
        ("/repo/c", manifest("[project]\nname = 'c'\n")),
    ];
    let combined = super::interpreter::requires_python_of(
        members
            .iter()
            .map(|(root, manifest)| (std::path::Path::new(root), &**manifest)),
    )
    .expect("the ranges parse")
    .expect("a range was declared");
    assert_eq!(combined.to_string(), ">=3.10, !=3.15");
    assert!(combined.contains(&"3.14.0".parse().unwrap()));
    assert!(!combined.contains(&"3.15.0".parse().unwrap()));

    let undeclared = super::interpreter::requires_python_of(std::iter::once((
        std::path::Path::new("/repo/c"),
        &*members[2].1,
    )))
    .expect("nothing to parse");
    assert!(undeclared.is_none());
}

/// A command run in a member of a shared environment finds the
/// environment at the workspace root; one run in a member of a workspace
/// that shares none, or where no workspace is configured, uses the
/// directory's own.
#[test]
fn a_command_uses_the_environment_its_workspace_shares_or_its_own() {
    let workspace = tempfile::tempdir().expect("workspace directory");
    let member = workspace.path().join("packages/app");
    std::fs::create_dir_all(&member).expect("member directory");
    std::fs::create_dir(workspace.path().join(".venv")).expect("root environment");
    let own = member.join(".venv");
    let root_manifest = workspace.path().join("pyproject.toml");
    std::fs::write(&root_manifest, "[tool.uv.workspace]\nmembers = ['packages/*']\n")
        .expect("unshared workspace");
    assert_eq!(super::environment_dir(Some(workspace.path()), &member), own);

    std::fs::write(&root_manifest, SHARED_ROOT).expect("shared workspace");
    assert_eq!(
        super::environment_dir(Some(workspace.path()), &member),
        workspace.path().join(".venv"),
    );
    assert_eq!(super::environment_dir(None, &member), own, "no workspace to share one");
    let excluded = workspace.path().join("packages/tool");
    assert_eq!(super::environment_dir(Some(workspace.path()), &excluded), excluded.join(".venv"));
    assert_eq!(
        super::environment_dir(Some(workspace.path()), workspace.path()),
        workspace.path().join(".venv"),
    );
}
