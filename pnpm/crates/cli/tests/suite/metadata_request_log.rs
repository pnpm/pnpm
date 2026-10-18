//! The registry requests of three installs that resolve the same project
//! against an unchanged registry, sharing one metadata cache. The cases and
//! their metadata request logs are shared with pnpm v11, see the table's
//! `description`.

use assert_cmd::prelude::*;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    registry::RecordedRequest,
};
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::{collections::HashMap, fs, io};

const CASE_TABLE: &str =
    include_str!("../../../../../pnpr/.fixtures/request-logs/metadata-three-installs.json");

const PKG_TARBALL: &str = "GET /@pnpm.e2e/pkg-with-1-dep/-/pkg-with-1-dep-100.1.0.tgz 200";
const DEP_TARBALL: &str =
    "GET /@pnpm.e2e/dep-of-pkg-with-1-dep/-/dep-of-pkg-with-1-dep-100.1.0.tgz 200";

/// Install 1 also downloads both tarballs, which the store keeps for installs
/// 2 and 3. The table leaves tarballs out: in pnpm v11's test they bypass the
/// recording proxy.
#[test]
fn every_case_of_the_shared_table() {
    let table: Value = serde_json::from_str(CASE_TABLE).expect("parse the case table");
    for case in table["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("case name");
        let settings: Vec<String> = case["settings"]
            .as_object()
            .expect("case settings")
            .iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect();
        let args: Vec<&str> = case["args"]
            .as_array()
            .expect("case args")
            .iter()
            .map(|arg| arg.as_str().expect("case arg"))
            .collect();
        let expected: Vec<Vec<String>> =
            serde_json::from_value(case["installs"].clone()).expect("case installs");

        let (metadata, tarballs) = three_installs(&table["dependencies"], &settings, &args);
        assert_eq!(metadata, expected, "{name}");
        assert_eq!(tarballs, [vec![DEP_TARBALL, PKG_TARBALL], vec![], vec![]], "{name}");
    }
}

type InstallLogs = Vec<Vec<String>>;

/// Each install's metadata requests as the table spells them, and its
/// tarball paths, both sorted: concurrent fetches finish in any order.
fn three_installs(
    dependencies: &Value,
    settings: &[String],
    args: &[&str],
) -> (InstallLogs, InstallLogs) {
    let CommandTempCwd {
        mut pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_recording_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let manifest = serde_json::json!({ "dependencies": dependencies });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml, format!("{yaml}{}\n", settings.join("\n")))
        .expect("write pnpm-workspace.yaml");
    pacquet.arg("install").args(args);

    let installs: Vec<Vec<RecordedRequest>> = (0..3)
        .map(|_| {
            // Remove what the last install wrote, so this one resolves again.
            remove_if_present(fs::remove_dir_all(workspace.join("node_modules")));
            remove_if_present(fs::remove_file(workspace.join("pnpm-lock.yaml")));
            pacquet.assert().success();
            mock_instance
                .take_requests()
                .into_iter()
                .map(registry_relative)
                .collect()
        })
        .collect();
    drop(root);

    let etags: HashMap<(&str, &str), &str> = installs
        .iter()
        .flatten()
        .filter(|request| request.status == 200)
        .filter_map(|request| {
            Some(((request.path.as_str(), document(request)), request.etag.as_deref()?))
        })
        .collect();
    let log_of = |requests: &Vec<RecordedRequest>, tarballs: bool| -> Vec<String> {
        let mut log: Vec<String> = requests
            .iter()
            .filter(|request| is_tarball(request) == tarballs)
            .map(|request| {
                if tarballs {
                    format!("{} {} {}", request.method, request.path, request.status)
                } else {
                    describe(request, &etags)
                }
            })
            .collect();
        log.sort();
        log
    };
    let metadata = installs
        .iter()
        .map(|requests| log_of(requests, false))
        .collect();
    let tarballs = installs
        .iter()
        .map(|requests| log_of(requests, true))
        .collect();
    (metadata, tarballs)
}

/// The table spells paths relative to the npm registry URL.
fn registry_relative(mut request: RecordedRequest) -> RecordedRequest {
    if let Some(path) = request.path.strip_prefix("/npm") {
        request.path = path.to_string();
    }
    request
}

fn remove_if_present(result: io::Result<()>) {
    if let Err(error) = result {
        assert_eq!(error.kind(), io::ErrorKind::NotFound, "clean the workspace: {error}");
    }
}

fn is_tarball(request: &RecordedRequest) -> bool {
    request.path.ends_with(".tgz")
}

fn document(request: &RecordedRequest) -> &'static str {
    match request.accept.as_deref() {
        Some(accept) if accept.starts_with("application/vnd.npm.install-v1+json") => "abbreviated",
        _ => "full",
    }
}

/// A request sending some other `ETag` reads `other-etag`, which no case
/// expects.
fn describe(request: &RecordedRequest, etags: &HashMap<(&str, &str), &str>) -> String {
    let RecordedRequest {
        method,
        path,
        if_none_match,
        if_modified_since,
        status,
        ..
    } = request;
    let document = document(request);
    let mut validators = String::new();
    if let Some(sent) = if_none_match {
        let own = etags.get(&(path.as_str(), document)) == Some(&sent.as_str());
        validators.push_str(if own { " etag" } else { " other-etag" });
    }
    if if_modified_since.is_some() {
        validators.push_str(" since");
    }
    format!("{method} {path} {document}{validators} {status}")
}
