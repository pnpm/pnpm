//! The registry requests of three installs that resolve the same project
//! against an unchanged registry, sharing one metadata cache. Only the first
//! install may download a body.

use assert_cmd::prelude::*;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    registry::RecordedRequest,
};
use pretty_assertions::assert_eq;
use std::{collections::HashMap, fs, io};

const PKG: &str = "/@pnpm.e2e%2Fpkg-with-1-dep";
const DEP: &str = "/@pnpm.e2e%2Fdep-of-pkg-with-1-dep";
const PKG_TARBALL: &str = "/@pnpm.e2e/pkg-with-1-dep/-/pkg-with-1-dep-100.1.0.tgz";
const DEP_TARBALL: &str = "/@pnpm.e2e/dep-of-pkg-with-1-dep/-/dep-of-pkg-with-1-dep-100.1.0.tgz";

/// Each install's requests, sorted: concurrent fetches finish in any order.
fn three_installs(settings: &str, args: &[&str]) -> [Vec<String>; 3] {
    let CommandTempCwd {
        mut pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_recording_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let manifest =
        serde_json::json!({ "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "^100.0.0" } });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml, format!("{yaml}{settings}")).expect("write pnpm-workspace.yaml");
    pacquet.arg("install").args(args);

    let installs = [(); 3].map(|()| {
        // Remove what the last install wrote, so this one resolves again.
        remove_if_present(fs::remove_dir_all(workspace.join("node_modules")));
        remove_if_present(fs::remove_file(workspace.join("pnpm-lock.yaml")));
        pacquet.assert().success();
        mock_instance.take_requests()
    });
    drop(root);

    let etags: HashMap<(String, &str), String> = installs
        .iter()
        .flatten()
        .filter(|request| request.status == 200)
        .filter_map(|request| {
            let etag = request.etag.clone()?;
            Some(((request.path.clone(), document(request)), etag))
        })
        .collect();
    installs.map(|requests| {
        let mut log: Vec<String> = requests
            .iter()
            .map(|request| describe(request, &etags))
            .collect();
        log.sort();
        log
    })
}

fn remove_if_present(result: io::Result<()>) {
    if let Err(error) = result {
        assert_eq!(error.kind(), io::ErrorKind::NotFound, "clean the workspace: {error}");
    }
}

fn document(request: &RecordedRequest) -> &'static str {
    match request.accept.as_deref() {
        Some(accept) if accept.starts_with("application/vnd.npm.install-v1+json") => "abbreviated",
        _ => "full",
    }
}

/// A metadata request reads `etag` when it sends the `ETag` this registry
/// gave the same representation, `other-etag` for any other value, and
/// `since` when it sends `If-Modified-Since`.
fn describe(request: &RecordedRequest, etags: &HashMap<(String, &str), String>) -> String {
    let RecordedRequest {
        method,
        path,
        if_none_match,
        if_modified_since,
        status,
        ..
    } = request;
    if path.ends_with(".tgz") {
        return format!("{method} {path} {status}");
    }
    let document = document(request);
    let mut validators = String::new();
    if let Some(sent) = if_none_match {
        let own = etags.get(&(path.clone(), document)) == Some(sent);
        validators.push_str(if own { " etag" } else { " other-etag" });
    }
    if if_modified_since.is_some() {
        validators.push_str(" since");
    }
    format!("{method} {path} {document}{validators} {status}")
}

fn first_install(document: &str) -> Vec<String> {
    let mut log = vec![
        format!("GET {DEP} {document} 200"),
        format!("GET {DEP_TARBALL} 200"),
        format!("GET {PKG} {document} 200"),
        format!("GET {PKG_TARBALL} 200"),
    ];
    log.sort();
    log
}

/// `request` is the document kind and the validators, as [`describe`]
/// spells them.
fn revalidation(request: &str) -> Vec<String> {
    vec![format!("GET {DEP} {request} 304"), format!("GET {PKG} {request} 304")]
}

#[test]
fn abbreviated_metadata_is_revalidated_with_its_etag() {
    let [first, second, third] = three_installs("minimumReleaseAge: 0\n", &[]);
    assert_eq!(first, first_install("abbreviated"));
    assert_eq!(second, revalidation("abbreviated etag"));
    assert_eq!(third, revalidation("abbreviated etag"));
}

#[test]
fn full_metadata_is_revalidated_with_its_etag() {
    let [first, second, third] =
        three_installs("minimumReleaseAge: 0\ntrustPolicy: no-downgrade\n", &[]);
    assert_eq!(first, first_install("full"));
    assert_eq!(second, revalidation("full etag since"));
    assert_eq!(third, revalidation("full etag since"));
}

/// A mirror written after the `minimumReleaseAge` cutoff cannot be missing a
/// mature version, so it answers without a request.
#[test]
fn a_mirror_written_after_the_release_age_cutoff_answers_without_a_request() {
    let [first, second, third] = three_installs("minimumReleaseAge: 1440\n", &[]);
    assert_eq!(first, first_install("abbreviated"));
    assert_eq!(second, Vec::<String>::new());
    assert_eq!(third, Vec::<String>::new());
}

#[test]
fn prefer_offline_answers_from_the_mirror_without_a_request() {
    let [first, second, third] = three_installs("minimumReleaseAge: 0\n", &["--prefer-offline"]);
    assert_eq!(first, first_install("abbreviated"));
    assert_eq!(second, Vec::<String>::new());
    assert_eq!(third, Vec::<String>::new());
}
