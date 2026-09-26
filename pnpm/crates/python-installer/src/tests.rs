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

fn config_for_pnpr(server: &str, test: &str) -> Config {
    let mut config = Config::new();
    config.pnpr_server = Some(format!("{server}/{test}"));
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
        .mock("GET", "/resolves-python/-/pnpr")
        .with_body(handshake_body(&["npm", "pypi"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/resolves-python/-/pnpr/v0/resolve")
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
        &config_for_pnpr(&server.url(), "resolves-python"),
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
        .mock("GET", "/without-python/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/without-python/-/pnpr/v0/resolve")
        .expect(0)
        .create_async()
        .await;

    let resolved = resolve_via_pnpr(
        &config_for_pnpr(&server.url(), "without-python"),
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
        .mock("GET", "/offline/-/pnpr")
        .expect(0)
        .create_async()
        .await;
    let mut config = config_for_pnpr(&server.url(), "offline");
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
const OTHER: &str = "[project]\nname = 'other'\nversion = '1.0'\n";

/// The members of a workspace whose root asks for a shared environment
/// install into the root; a project the declaration excludes, and one
/// outside it, keep their own.
#[test]
fn a_shared_workspace_groups_its_members_and_leaves_the_others_alone() {
    let (workspace, roots) = workspace_of(&[
        ("/repo", SHARED_ROOT),
        ("/repo/packages/a", MEMBER),
        ("/repo/packages/b", OTHER),
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
fn two_shared_members_declaring_one_distribution_are_refused() {
    let error = workspace_of(&[
        ("/repo", SHARED_ROOT),
        ("/repo/packages/a", MEMBER),
        ("/repo/packages/b", MEMBER),
    ])
    .err()
    .expect("refused");
    assert!(error.to_string().contains("both declare `member`"), "{error}");
    // Unshared, each installs its own environment, so the names may clash.
    workspace_of(&[
        ("/repo", "[tool.uv.workspace]\nmembers = ['packages/*']\n"),
        ("/repo/packages/a", MEMBER),
        ("/repo/packages/b", MEMBER),
    ])
    .expect("two environments");
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

const UNSHARED_ROOT: &str = "[tool.uv.workspace]\nmembers = ['libs/*']\n";

/// A command run in a member of a shared environment, or anywhere under
/// it, finds the environment at the workspace root. One run in a member
/// of a workspace that shares none, in a project the declaration
/// excludes, or where no workspace is configured, uses the project's own.
/// A nested workspace is the one its own members belong to.
#[test]
fn a_command_uses_the_environment_its_project_shares_or_its_own() {
    let workspace = tempfile::tempdir().expect("workspace directory");
    // The lookup answers with links resolved, and a temporary directory
    // may be reached through one.
    let root = dunce::canonicalize(workspace.path()).expect("canonical workspace");
    let root = root.as_path();
    let environment_of = |dir: &std::path::Path| super::environment_dir(Some(root), dir);
    for project in ["packages/app", "packages/tool", "packages/nested/libs/x", "packages/inner"] {
        std::fs::create_dir_all(root.join(project).join("src")).expect("project directory");
        std::fs::write(root.join(project).join("pyproject.toml"), MEMBER).expect("member");
    }
    let app = root.join("packages/app");
    std::fs::write(root.join("pyproject.toml"), UNSHARED_ROOT.replace("libs", "packages"))
        .expect("unshared workspace");
    assert_eq!(environment_of(&app), app.join(".venv"));

    std::fs::write(root.join("pyproject.toml"), SHARED_ROOT).expect("shared workspace");
    assert_eq!(environment_of(&app), root.join(".venv"));
    assert_eq!(environment_of(&app.join("src")), root.join(".venv"), "under a member");
    assert_eq!(super::environment_dir(None, &app), app.join(".venv"), "no workspace");
    let tool = root.join("packages/tool");
    assert_eq!(environment_of(&tool), tool.join(".venv"), "excluded");
    assert_eq!(environment_of(root), root.join(".venv"), "the root itself");

    std::fs::write(root.join("packages/nested/pyproject.toml"), UNSHARED_ROOT)
        .expect("nested unshared workspace");
    let nested_member = root.join("packages/nested/libs/x");
    assert_eq!(
        environment_of(&nested_member),
        nested_member.join(".venv"),
        "a member of the nearer workspace",
    );
    std::fs::write(root.join("packages/inner/pyproject.toml"), SHARED_ROOT)
        .expect("nested shared workspace");
    let inner = root.join("packages/inner");
    assert_eq!(environment_of(&inner), inner.join(".venv"), "its own workspace root");
}

#[test]
fn a_workspace_reached_through_a_link_still_shares_its_environment() {
    let outside = tempfile::tempdir().expect("outside directory");
    let real = outside.path().join("real");
    let member = real.join("packages/app");
    std::fs::create_dir_all(&member).expect("member directory");
    std::fs::write(real.join("pyproject.toml"), SHARED_ROOT).expect("shared workspace");
    std::fs::write(member.join("pyproject.toml"), MEMBER).expect("member");
    let link = outside.path().join("link");
    pnpm_fs::force_symlink_dir(&real, &link).expect("workspace link");
    let canonical = dunce::canonicalize(&real).expect("canonical workspace");

    assert_eq!(super::environment_dir(Some(&link), &member), canonical.join(".venv"));
    assert!(super::in_declared_workspace(&link, &member));
    assert!(!super::in_declared_workspace(&link, outside.path()), "outside the workspace");
}
