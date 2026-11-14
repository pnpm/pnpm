use crate::_utils::{importer_version, read_lockfile};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use pretty_assertions::assert_eq;
use std::{
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const FOO: &str = "@pnpm.e2e/foo";

struct LocalGitHttpServer {
    addr: std::net::SocketAddr,
    dir: TempDir,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl LocalGitHttpServer {
    fn start() -> Self {
        let dir = tempfile::tempdir().expect("git server dir");
        let root = dir.path().to_path_buf();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }
                serve_git_file(&root, stream);
            }
        });
        Self { addr, dir, stop, handle: Some(handle) }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn create_action_repo(&self, repo: &str, tags: &[&str]) -> Vec<String> {
        let work = self.dir
            .path()
            .join(format!("work/{repo}"));
        let bare = self.dir
            .path()
            .join(format!("{repo}.git"));
        fs::create_dir_all(&work).expect("create work dir");
        if let Some(parent) = bare.parent() {
            fs::create_dir_all(parent).expect("create bare parent");
        }
        let no_config = self.dir.path().join(".no-global-git-config");
        let run_git = |cwd: &Path, args: &[&str]| -> String {
            let output = Command::new("git")
                .with_current_dir(cwd)
                .with_env("GIT_CONFIG_GLOBAL", &no_config)
                .with_env("GIT_CONFIG_NOSYSTEM", "1")
                .with_args(["-c", "user.name=pnpm", "-c", "user.email=pnpm@example.com"])
                .with_args(args)
                .output()
                .expect("run git");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr),
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        run_git(&work, &["init", "--quiet"]);
        let mut commits = Vec::with_capacity(tags.len());
        for tag in tags {
            run_git(&work, &["commit", "--quiet", "--allow-empty", "-m", tag]);
            run_git(&work, &["tag", tag]);
            commits.push(run_git(&work, &["rev-parse", "HEAD"]));
        }
        run_git(
            &work,
            &["clone", "--quiet", "--bare", work.to_str().unwrap(), bare.to_str().unwrap()],
        );
        run_git(&bare, &["update-server-info"]);
        commits
    }
}

impl Drop for LocalGitHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve_git_file(root: &Path, mut stream: TcpStream) {
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
    }
    let raw_target = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/");
    let rel = raw_target
        .split('?')
        .next()
        .unwrap_or("/")
        .trim_start_matches('/');
    let file_path = root.join(rel);
    if !rel.contains("..")
        && let Ok(bytes) = fs::read(&file_path)
    {
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len(),
        );
        let _ = stream.write_all(&bytes);
    } else {
        let _ = write!(
            stream,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
    }
}

fn setup() -> (TempDir, PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut existing = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    existing.push_str("minimumReleaseAge: 0\n");
    fs::write(&workspace_yaml, existing).expect("write pnpm-workspace.yaml");
    (root, workspace, npmrc_info)
}

fn pacquet(
    workspace: &Path,
    git_server_url: &str,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
        .with_env("GITHUB_SERVER_URL", git_server_url)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest = format!(
        r#"{{ "name": "test-gh-actions", "version": "1.0.0", "dependencies": {dependencies} }}"#,
    );
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

#[test]
fn update_with_purl_selector_and_include_github_actions_updates_npm_package_only() {
    let (root, workspace, anchor) = setup();
    let git_server = LocalGitHttpServer::start();
    let checkout_shas = git_server.create_action_repo("actions/checkout", &["v4.1.0", "v4.2.0"]);

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0", "{FOO}": "100.0.0" }}"#));
    pacquet(&workspace, &git_server.url(), ["install"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "^100.0.0" }}"#));

    let workflows = workspace.join(".github/workflows");
    fs::create_dir_all(&workflows).expect("mkdir workflows");
    let workflow = workflows.join("ci.yml");
    let workflow_source = format!(
        "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@{} # v4\n",
        checkout_shas[0],
    );
    fs::write(&workflow, &workflow_source).expect("write workflow");

    pacquet(
        &workspace,
        &git_server.url(),
        ["update", "pkg:npm/%40pnpm.e2e/dep-of-pkg-with-1-dep", "--include-github-actions"],
    )
    .assert()
    .success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, ".", DEP), "100.1.0");
    assert_eq!(importer_version(&lockfile, ".", FOO), "100.0.0");
    assert_eq!(fs::read_to_string(&workflow).expect("read workflow"), workflow_source);

    drop((root, anchor));
}

#[test]
fn update_with_negated_action_selector_updates_packages_and_unexcluded_actions() {
    let (root, workspace, anchor) = setup();
    let git_server = LocalGitHttpServer::start();
    let checkout_shas = git_server.create_action_repo("actions/checkout", &["v4.1.0", "v4.2.0"]);
    let cache_shas = git_server.create_action_repo("actions/cache", &["v4.1.0", "v4.2.0"]);

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, &git_server.url(), ["install"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));

    let workflows = workspace.join(".github/workflows");
    fs::create_dir_all(&workflows).expect("mkdir workflows");
    let workflow = workflows.join("ci.yml");
    fs::write(
        &workflow,
        format!(
            concat!(
                "jobs:\n  test:\n    steps:\n",
                "      - uses: actions/checkout@{} # v4\n",
                "      - uses: actions/cache/restore@{} # v4\n",
                "      - uses: actions/cache/save@{} # v4\n",
            ),
            checkout_shas[0], cache_shas[0], cache_shas[0],
        ),
    )
    .expect("write workflow");

    pacquet(
        &workspace,
        &git_server.url(),
        ["update", "!actions/checkout", "!actions/cache/save", "--include-github-actions"],
    )
    .assert()
    .success();

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, ".", DEP), "100.1.0");
    assert_eq!(
        fs::read_to_string(&workflow).expect("read workflow"),
        format!(
            concat!(
                "jobs:\n  test:\n    steps:\n",
                "      - uses: actions/checkout@{} # v4\n",
                "      - uses: actions/cache/restore@{} # v4.2.0\n",
                "      - uses: actions/cache/save@{} # v4\n",
            ),
            checkout_shas[0], cache_shas[1], cache_shas[0],
        ),
    );

    drop((root, anchor));
}

#[test]
fn outdated_json_reports_all_distinct_workflow_versions_of_same_action() {
    let (root, workspace, anchor) = setup();
    let git_server = LocalGitHttpServer::start();
    let checkout_shas = git_server.create_action_repo(
        "actions/checkout",
        &["v3.6.0", "v3.6.1", "v4.1.0", "v4.2.0", "v5.0.0"],
    );

    write_manifest(&workspace, "{}");
    let workflows = workspace.join(".github/workflows");
    fs::create_dir_all(&workflows).expect("mkdir workflows");
    fs::write(
        workflows.join("ci.yml"),
        format!(
            "jobs:\n  test:\n    steps:\n      - uses: actions/checkout@{} # v4\n",
            checkout_shas[2],
        ),
    )
    .expect("write ci.yml");
    fs::write(
        workflows.join("release.yml"),
        format!(
            "jobs:\n  release:\n    steps:\n      - uses: actions/checkout@{} # v3\n",
            checkout_shas[0],
        ),
    )
    .expect("write release.yml");

    let output =
        pacquet(&workspace, &git_server.url(), ["outdated", "--include-github-actions", "--json"])
            .output()
            .expect("run outdated");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let report: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&output.stdout))
        .expect("parse outdated JSON");
    assert_eq!(report["actions/checkout@3.6.0 (github action)"]["current"], "3.6.0");
    assert_eq!(report["actions/checkout@3.6.0 (github action)"]["wanted"], "3.6.1");
    assert_eq!(report["actions/checkout@3.6.0 (github action)"]["latest"], "5.0.0");
    assert_eq!(report["actions/checkout@4.1.0 (github action)"]["current"], "4.1.0");
    assert_eq!(report["actions/checkout@4.1.0 (github action)"]["wanted"], "4.2.0");
    assert_eq!(report["actions/checkout@4.1.0 (github action)"]["latest"], "5.0.0");

    drop((root, anchor));
}
