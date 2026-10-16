//! `lockBuildTools`: the lockfile records the tools that prepare a
//! git-hosted dependency, and later installs prepare it with exactly those.
//!
//! The package manager is Yarn Classic from the mocked registry, which
//! proxies the real bytes: pnpm verifies a package manager it provisions.

use super::{
    CommandTempCwd, GitRepoFixture, Lockfile, LockfileResolution, allow_builds,
    append_workspace_yaml_key, assert_eq, assert_success, fs, pnpm_at, read_lockfile,
    write_dependencies,
};
use pnpm_lockfile::PackageKey;
use std::path::Path;

/// A repository that pins Yarn 1.22.22 and records, from its `prepare`
/// script, the user agent it ran under and the `node` that ran it.
fn pins_yarn_repo(root: &Path, extra_manifest: &str) -> (GitRepoFixture, String) {
    let repo = GitRepoFixture::init(root, "pins-yarn");
    repo.write_file(
        "package.json",
        &format!(
            r#"{{"name":"pins-yarn","version":"1.0.0","main":"index.js","packageManager":"yarn@1.22.22","scripts":{{"prepare":"node record.js"}}{extra_manifest}}}"#,
        ),
    );
    repo.write_file("index.js", "module.exports = 'ok'\n");
    repo.write_file(
        "record.js",
        "require('fs').writeFileSync('prepared-by.txt', process.env.npm_config_user_agent || '')\n",
    );
    repo.write_file("yarn.lock", "# yarn lockfile v1\n");
    let commit = repo.commit("init");
    let spec = repo.git_url_at(&commit);
    (repo, spec)
}

/// `pnpm install` with every directory a provisioned package manager could
/// land in pinned under `root`.
fn install(workspace: &Path, root: &Path, args: &[&str]) -> std::process::Output {
    pnpm_at(workspace)
        .arg("install")
        .args(args)
        .env("PNPM_HOME", root.join("pnpm-home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CACHE_HOME", root.join("cache-home"))
        .env("YARN_IGNORE_PATH", "1")
        .output()
        .expect("run pnpm install")
}

fn prepared_by(workspace: &Path) -> String {
    fs::read_to_string(workspace.join("node_modules/pins-yarn/prepared-by.txt"))
        .expect("the dependency's prepare script should have run")
}

fn snapshot_key(spec: &str) -> PackageKey {
    format!("pins-yarn@{spec}").parse().expect("parse the snapshot key")
}

fn tool_key() -> PackageKey {
    "yarn@tool:1.22.22".parse().expect("parse the tool key")
}

fn set_up(root: &Path, workspace: &Path, extra_manifest: &str) -> String {
    let (_repo, spec) = pins_yarn_repo(root, extra_manifest);
    write_dependencies(workspace, &[("pins-yarn", &spec)]);
    allow_builds(workspace, &[&format!("pins-yarn@{spec}")]);
    append_workspace_yaml_key(workspace, "lockBuildTools", true);
    spec
}

#[test]
fn the_package_manager_a_git_dependency_pins_is_recorded_and_provisioned() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let spec = set_up(root.path(), &workspace, "");

    let output = install(&workspace, root.path(), &[]);
    dbg!(&output);
    assert_success(&output);

    assert!(prepared_by(&workspace).starts_with("yarn/1.22.22"));
    let lockfile: Lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(
        lockfile.settings.as_ref().and_then(|settings| settings.lock_build_tools),
        Some(true),
    );
    let snapshots = lockfile.snapshots.as_ref().expect("snapshots");
    let tools: Vec<_> = snapshots[&snapshot_key(&spec)]
        .build_dependency_keys()
        .map(|(alias, key)| (alias.to_string(), key.to_string()))
        .collect();
    assert_eq!(tools, [("yarn".to_string(), "yarn@tool:1.22.22".to_string())]);
    let LockfileResolution::Tarball(tarball) =
        &lockfile.packages.as_ref().expect("packages")[&tool_key()].resolution
    else {
        panic!("Yarn Classic is recorded as a tarball");
    };
    assert!(tarball.tarball.ends_with("/yarn/-/yarn-1.22.22.tgz"), "{}", tarball.tarball);
    assert!(tarball.integrity.is_some());
    // The recorded Yarn ran, not one `pnpm dlx` resolved again.
    assert!(
        !root
            .path()
            .join("pnpm-home/package-manager-store")
            .exists(),
    );
    // A build tool is never installed into the project.
    assert_eq!(virtual_store_slots(&workspace, "yarn@"), Vec::<String>::new());
    assert!(!workspace.join("node_modules/yarn").exists());

    // A frozen install into an empty store prepares the dependency again,
    // from the record alone.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    let fresh_store = root.path().join("fresh-store");
    let store_arg = format!("--store-dir={}", fresh_store.display());
    let output = install(&workspace, root.path(), &["--frozen-lockfile", &store_arg]);
    dbg!(&output);
    assert_success(&output);
    assert!(prepared_by(&workspace).starts_with("yarn/1.22.22"));
    assert_eq!(read_lockfile(&workspace.join("pnpm-lock.yaml")), lockfile);

    drop((root, npmrc_info));
}

/// The virtual-store slots whose names start with `prefix`.
fn virtual_store_slots(workspace: &Path, prefix: &str) -> Vec<String> {
    fs::read_dir(workspace.join("node_modules/.pnpm"))
        .expect("read the virtual store")
        .map(|entry| {
            entry
                .expect("read a slot")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with(prefix))
        .collect()
}

/// A lockfile that points a recorded tool at a file of its own is refused
/// before anything runs: a tool is only ever its own published artifact.
#[test]
fn a_recorded_tool_must_be_its_own_artifact() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    set_up(root.path(), &workspace, "");
    assert_success(&install(&workspace, root.path(), &[]));

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut lockfile = read_lockfile(&lockfile_path);
    let packages = lockfile.packages.as_mut().expect("packages");
    let LockfileResolution::Tarball(tarball) = &mut packages
        .get_mut(&tool_key())
        .expect("tool")
        .resolution
    else {
        panic!("Yarn Classic is recorded as a tarball");
    };
    tarball.tarball = "file:./not-yarn.tgz".to_string();
    tarball.integrity = None;
    fs::write(&lockfile_path, lockfile.to_yaml_string().expect("serialize"))
        .expect("write lockfile");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let fresh_store = root.path().join("fresh-store");
    let store_arg = format!("--store-dir={}", fresh_store.display());
    let output = install(&workspace, root.path(), &["--frozen-lockfile", &store_arg]);
    dbg!(&output);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_RESOLUTION_SHAPE_MISMATCH"), "{stderr}");
    assert!(!workspace.join("node_modules/pins-yarn/prepared-by.txt").exists());

    drop((root, npmrc_info));
}

#[test]
fn a_recorded_tool_whose_bytes_do_not_match_is_not_run() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    set_up(root.path(), &workspace, "");
    assert_success(&install(&workspace, root.path(), &[]));

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut lockfile = read_lockfile(&lockfile_path);
    let packages = lockfile.packages.as_mut().expect("packages");
    let LockfileResolution::Tarball(tarball) = &mut packages
        .get_mut(&tool_key())
        .expect("tool")
        .resolution
    else {
        panic!("Yarn Classic is recorded as a tarball");
    };
    // The integrity of another package entirely.
    tarball.integrity = Some(
        "sha512-qkS8GZYTlbRZk+VYLiR8aaVPxgUkqMf/sGWbdPD9cNHdtx7dGMH9Gy7JTvZY0H7YyR9vxpqjOmPBgxoY2pslOw=="
            .parse()
            .expect("parse integrity"),
    );
    fs::write(&lockfile_path, lockfile.to_yaml_string().expect("serialize"))
        .expect("write lockfile");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let fresh_store = root.path().join("fresh-store");
    let store_arg = format!("--store-dir={}", fresh_store.display());
    append_workspace_yaml_key(&workspace, "fetchRetries", 0);
    let output = install(&workspace, root.path(), &["--frozen-lockfile", &store_arg]);
    dbg!(&output);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_TARBALL_INTEGRITY"), "{stderr}");
    assert!(!workspace.join("node_modules/pins-yarn/prepared-by.txt").exists());

    drop((root, npmrc_info));
}

/// Turning the setting on makes a lockfile without records stale, so a
/// frozen install cannot resolve the tools live.
#[test]
fn opting_in_needs_a_lockfile_with_the_records() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let (_repo, spec) = pins_yarn_repo(root.path(), "");
    write_dependencies(&workspace, &[("pins-yarn", &spec)]);
    allow_builds(&workspace, &[&format!("pins-yarn@{spec}")]);
    assert_success(&install(&workspace, root.path(), &["--lockfile-only"]));
    append_workspace_yaml_key(&workspace, "lockBuildTools", true);

    let output = install(&workspace, root.path(), &["--frozen-lockfile"]);
    dbg!(&output);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_LOCKFILE_CONFIG_MISMATCH"), "{stderr}");
    assert!(stderr.contains("lockBuildTools"), "{stderr}");

    assert_success(&install(&workspace, root.path(), &[]));
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert!(
        lockfile.packages
            .as_ref()
            .expect("packages")
            .contains_key(&tool_key()),
    );

    drop((root, npmrc_info));
}

/// A dependency that pins its development Node.js with `onFail: download`
/// is prepared on that Node.js. The mirror serves a `node` that marks its
/// use and hands over to the host's.
#[test]
#[cfg_attr(windows, ignore = "the mock Node.js is a POSIX shell script")]
fn the_node_a_git_dependency_pins_is_recorded_and_runs_the_build() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let version = "24.0.0-rc.4";
    let marker = root.path().join("provisioned-node-ran");
    let mut server = mockito::Server::new();
    let _mocks = mock_node_release(&mut server, version, &marker);
    append_workspace_yaml_key(
        &workspace,
        "nodeDownloadMirrors",
        format!("{{ rc: '{}/' }}", server.url()),
    );
    let spec = set_up(
        root.path(),
        &workspace,
        &format!(
            r#","devEngines":{{"runtime":{{"name":"node","version":"{version}","onFail":"download"}}}}"#,
        ),
    );

    let output = install(&workspace, root.path(), &[]);
    dbg!(&output);
    assert_success(&output);

    assert!(marker.exists(), "the recorded Node.js should have run the build");
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let tools: Vec<_> = lockfile.snapshots.as_ref().expect("snapshots")[&snapshot_key(&spec)]
        .build_dependency_keys()
        .map(|(alias, key)| (alias.to_string(), key.to_string()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(
        tools,
        [
            ("node".to_string(), format!("node@tool:{version}")),
            ("yarn".to_string(), "yarn@tool:1.22.22".to_string()),
        ],
    );
    assert!(!workspace.join("node_modules/node").exists());

    drop((root, npmrc_info, server));
}

/// Serve one Node.js release whose `bin/node` touches `marker` and then
/// runs the host's `node`.
fn mock_node_release(
    server: &mut mockito::Server,
    version: &str,
    marker: &Path,
) -> Vec<mockito::Mock> {
    use sha2::Digest as _;
    use std::io::Write as _;

    let host_node = which::which("node").expect("the tests need Node.js");
    let script =
        format!("#!/bin/sh\n: > '{}'\nexec '{}' \"$@\"\n", marker.display(), host_node.display());
    let platform = pnpm_graph_hasher::host_platform();
    let arch = pnpm_graph_hasher::host_arch();
    let archive_name = format!("node-v{version}-{platform}-{arch}.tar.gz");
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o755);
    tar.append_data(
        &mut header,
        format!("node-v{version}-{platform}-{arch}/bin/node"),
        script.as_bytes(),
    )
    .expect("append bin/node");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&tar.into_inner().expect("finish the archive"))
        .expect("compress");
    let archive = encoder.finish().expect("finish compressing");
    let digest = format!("{:x}", sha2::Sha256::digest(&archive));
    vec![
        server
            .mock("GET", "/index.json")
            .with_body(format!(r#"[{{"version":"v{version}","lts":false}}]"#))
            .create(),
        server
            .mock("GET", format!("/v{version}/SHASUMS256.txt").as_str())
            .with_body(format!("{digest}  {archive_name}\n"))
            .create(),
        server
            .mock("GET", format!("/v{version}/{archive_name}").as_str())
            .with_body(archive)
            .create(),
    ]
}
