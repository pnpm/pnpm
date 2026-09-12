use super::{
    Arc, Matcher, Mutex, SECOND_STAGE_ID, STAGE_ID, assert_failure_with_code, assert_success, json,
    package_tarball, stage, staged_item_of, write_registry_config,
};

#[test]
fn approve_and_reject_send_the_configured_otp_and_stage_headers() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let approve_mock = server
        .mock("POST", format!("/-/stage/{STAGE_ID}/approve").as_str())
        .match_header("npm-auth-type", "web")
        .match_header("npm-command", "stage")
        .match_header("npm-otp", "123456")
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();
    let reject_mock = server
        .mock("DELETE", format!("/-/stage/{STAGE_ID}").as_str())
        .match_header("npm-auth-type", "web")
        .match_header("npm-command", "stage")
        .match_header("npm-otp", "123456")
        .with_status(204)
        .expect(1)
        .create();

    let approve = stage(dir.path(), &["approve", STAGE_ID, "--otp", "123456"]);
    approve_mock.assert();
    assert_success(&approve);
    assert_eq!(
        String::from_utf8_lossy(&approve.stdout),
        format!("Staged package {STAGE_ID} approved and published successfully.\n"),
    );

    let reject = stage(dir.path(), &["reject", STAGE_ID, "--otp", "123456"]);
    reject_mock.assert();
    assert_success(&reject);
    let stdout = String::from_utf8_lossy(&reject.stdout);
    assert!(
        stdout.contains(&format!("Staged package {STAGE_ID} has been rejected.")),
        "stdout: {stdout}",
    );
}

#[test]
fn approve_downloads_every_tarball_before_approving_any_stage_with_one_otp() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let events = Arc::new(Mutex::new(Vec::new()));
    let described_mocks: Vec<mockito::Mock> =
        [(STAGE_ID, "@scope/first"), (SECOND_STAGE_ID, "@scope/second")]
            .into_iter()
            .map(|(stage_id, package_name)| {
                server
                    .mock("GET", format!("/-/stage/{stage_id}").as_str())
                    .with_body(staged_item_of(stage_id, package_name).to_string())
                    .expect(1)
                    .create()
            })
            .collect();
    let tarball_mocks: Vec<mockito::Mock> =
        [(STAGE_ID, "@scope/first"), (SECOND_STAGE_ID, "@scope/second")]
            .into_iter()
            .map(|(stage_id, package_name)| {
                let events = Arc::clone(&events);
                let tarball = package_tarball(&json!({
                    "name": package_name,
                    "version": "1.0.0",
                }));
                server
                    .mock("GET", format!("/-/stage/{stage_id}/tarball").as_str())
                    .with_body_from_request(move |_| {
                        events.lock().expect("lock events").push(format!("tarball:{stage_id}"));
                        tarball.clone()
                    })
                    .expect(1)
                    .create()
            })
            .collect();
    let approve_mocks: Vec<mockito::Mock> = [STAGE_ID, SECOND_STAGE_ID]
        .into_iter()
        .map(|stage_id| {
            let events = Arc::clone(&events);
            server
                .mock("POST", format!("/-/stage/{stage_id}/approve").as_str())
                .match_header("npm-otp", "123456")
                .with_status(201)
                .with_body_from_request(move |_| {
                    events.lock().expect("lock events").push(format!("approve:{stage_id}"));
                    br#"{"ok":true}"#.to_vec()
                })
                .expect(1)
                .create()
        })
        .collect();

    let output = stage(
        dir.path(),
        &["approve", STAGE_ID, SECOND_STAGE_ID, "--otp", "123456", "--reporter=silent"],
    );

    for mock in described_mocks.iter().chain(&tarball_mocks).chain(&approve_mocks) {
        mock.assert();
    }
    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Approved 2 staged packages successfully.\n",
    );
    assert_eq!(
        *events.lock().expect("lock events"),
        [
            format!("tarball:{STAGE_ID}"),
            format!("tarball:{SECOND_STAGE_ID}"),
            format!("approve:{STAGE_ID}"),
            format!("approve:{SECOND_STAGE_ID}"),
        ],
    );
}

/// The dependency is approved first, so a dependency that never reaches the
/// registry keeps its dependent from being published against it.
#[test]
fn approve_skips_a_staged_package_whose_selected_dependency_could_not_be_approved() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let described_mocks: Vec<mockito::Mock> =
        [(STAGE_ID, "@scope/dependent"), (SECOND_STAGE_ID, "@scope/dependency")]
            .into_iter()
            .map(|(stage_id, package_name)| {
                server
                    .mock("GET", format!("/-/stage/{stage_id}").as_str())
                    .with_body(staged_item_of(stage_id, package_name).to_string())
                    .expect(1)
                    .create()
            })
            .collect();
    let tarball_mocks = [
        (
            STAGE_ID,
            package_tarball(&json!({
                "name": "@scope/dependent",
                "version": "1.0.0",
                "dependencies": { "@scope/dependency": "^1.0.0" },
            })),
        ),
        (
            SECOND_STAGE_ID,
            package_tarball(&json!({ "name": "@scope/dependency", "version": "1.0.0" })),
        ),
    ]
    .into_iter()
    .map(|(stage_id, tarball)| {
        server
            .mock("GET", format!("/-/stage/{stage_id}/tarball").as_str())
            .with_body(tarball)
            .expect(1)
            .create()
    })
    .collect::<Vec<_>>();
    let dependency_mock = server
        .mock("POST", format!("/-/stage/{SECOND_STAGE_ID}/approve").as_str())
        .with_status(409)
        .with_body(r#"{"error":"version already exists"}"#)
        .expect_at_least(1)
        .create();
    let dependent_mock =
        server.mock("POST", format!("/-/stage/{STAGE_ID}/approve").as_str()).expect(0).create();

    let output = stage(
        dir.path(),
        &["approve", STAGE_ID, SECOND_STAGE_ID, "--otp", "123456", "--reporter=silent"],
    );

    for mock in described_mocks.iter().chain(&tarball_mocks) {
        mock.assert();
    }
    dependency_mock.assert();
    dependent_mock.assert();
    assert!(!output.status.success(), "an incomplete approval batch must exit non-zero");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Approved 0 of 2 staged packages.\n");
}

#[test]
fn approve_derives_package_identity_and_aliases_from_tarballs() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let described_mocks =
        [(STAGE_ID, "@scope/not-the-dependent"), (SECOND_STAGE_ID, "@scope/not-the-dependency")]
            .into_iter()
            .map(|(stage_id, package_name)| {
                server
                    .mock("GET", format!("/-/stage/{stage_id}").as_str())
                    .with_body(staged_item_of(stage_id, package_name).to_string())
                    .expect(1)
                    .create()
            })
            .collect::<Vec<_>>();
    let tarball_mocks = [
        (
            STAGE_ID,
            package_tarball(&json!({
                "name": "@scope/dependent",
                "version": "1.0.0",
                "dependencies": {
                    "local-name": "npm:@scope/dependency@^1.0.0 || ",
                },
            })),
        ),
        (
            SECOND_STAGE_ID,
            package_tarball(&json!({ "name": "@scope/dependency", "version": "9.0.0" })),
        ),
    ]
    .into_iter()
    .map(|(stage_id, tarball)| {
        server
            .mock("GET", format!("/-/stage/{stage_id}/tarball").as_str())
            .with_body(tarball)
            .expect(1)
            .create()
    })
    .collect::<Vec<_>>();
    let approved = Arc::new(Mutex::new(Vec::new()));
    let approve_mocks = [STAGE_ID, SECOND_STAGE_ID]
        .into_iter()
        .map(|stage_id| {
            let approved = Arc::clone(&approved);
            server
                .mock("POST", format!("/-/stage/{stage_id}/approve").as_str())
                .with_status(201)
                .with_body_from_request(move |_| {
                    approved.lock().expect("lock approvals").push(stage_id);
                    br#"{"ok":true}"#.to_vec()
                })
                .expect(1)
                .create()
        })
        .collect::<Vec<_>>();

    let output = stage(
        dir.path(),
        &["approve", STAGE_ID, SECOND_STAGE_ID, "--otp", "123456", "--reporter=silent"],
    );

    for mock in described_mocks.iter().chain(&tarball_mocks).chain(&approve_mocks) {
        mock.assert();
    }
    assert_success(&output);
    assert_eq!(*approved.lock().expect("lock approvals"), [SECOND_STAGE_ID, STAGE_ID]);
}

#[test]
fn approve_does_not_bind_an_npm_alias_tag_to_a_selected_version() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let described_mocks = [(STAGE_ID, "@scope/dependent"), (SECOND_STAGE_ID, "@scope/dependency")]
        .into_iter()
        .map(|(stage_id, package_name)| {
            server
                .mock("GET", format!("/-/stage/{stage_id}").as_str())
                .with_body(staged_item_of(stage_id, package_name).to_string())
                .expect(1)
                .create()
        })
        .collect::<Vec<_>>();
    let tarball_mocks = [
        (
            STAGE_ID,
            package_tarball(&json!({
                "name": "@scope/dependent",
                "version": "1.0.0",
                "dependencies": {
                    "local-name": "npm:@scope/dependency@latest",
                },
            })),
        ),
        (
            SECOND_STAGE_ID,
            package_tarball(&json!({ "name": "@scope/dependency", "version": "1.0.0" })),
        ),
    ]
    .into_iter()
    .map(|(stage_id, tarball)| {
        server
            .mock("GET", format!("/-/stage/{stage_id}/tarball").as_str())
            .with_body(tarball)
            .expect(1)
            .create()
    })
    .collect::<Vec<_>>();
    let approved = Arc::new(Mutex::new(Vec::new()));
    let approve_mocks = [STAGE_ID, SECOND_STAGE_ID]
        .into_iter()
        .map(|stage_id| {
            let approved = Arc::clone(&approved);
            server
                .mock("POST", format!("/-/stage/{stage_id}/approve").as_str())
                .with_status(201)
                .with_body_from_request(move |_| {
                    approved.lock().expect("lock approvals").push(stage_id);
                    br#"{"ok":true}"#.to_vec()
                })
                .expect(1)
                .create()
        })
        .collect::<Vec<_>>();

    let output = stage(
        dir.path(),
        &["approve", STAGE_ID, SECOND_STAGE_ID, "--otp", "123456", "--reporter=silent"],
    );

    for mock in described_mocks.iter().chain(&tarball_mocks).chain(&approve_mocks) {
        mock.assert();
    }
    assert_success(&output);
    assert_eq!(*approved.lock().expect("lock approvals"), [STAGE_ID, SECOND_STAGE_ID]);
}

#[test]
fn approve_rejects_duplicate_package_versions_before_approving_the_batch() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let described_mocks = [STAGE_ID, SECOND_STAGE_ID]
        .into_iter()
        .map(|stage_id| {
            server
                .mock("GET", format!("/-/stage/{stage_id}").as_str())
                .with_body(staged_item_of(stage_id, "@scope/duplicate").to_string())
                .expect(1)
                .create()
        })
        .collect::<Vec<_>>();
    let tarball = package_tarball(&json!({
        "name": "@scope/duplicate",
        "version": "1.0.0",
    }));
    let tarball_mocks = [STAGE_ID, SECOND_STAGE_ID]
        .into_iter()
        .map(|stage_id| {
            server
                .mock("GET", format!("/-/stage/{stage_id}/tarball").as_str())
                .with_body(tarball.clone())
                .expect(1)
                .create()
        })
        .collect::<Vec<_>>();
    let approve_mock = server.mock("POST", Matcher::Any).expect(0).create();

    let output = stage(
        dir.path(),
        &["approve", STAGE_ID, SECOND_STAGE_ID, "--otp", "123456", "--reporter=silent"],
    );

    for mock in described_mocks.iter().chain(&tarball_mocks) {
        mock.assert();
    }
    approve_mock.assert();
    assert_failure_with_code(&output, "ERR_PNPM_STAGE_DUPLICATE_PACKAGE");
}

#[test]
fn approve_sends_one_request_for_a_repeated_stage_id() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let read_mock = server.mock("GET", Matcher::Any).expect(0).create();
    let approve_mock = server
        .mock("POST", format!("/-/stage/{STAGE_ID}/approve").as_str())
        .match_header("npm-otp", "123456")
        .with_status(201)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();

    let output =
        stage(dir.path(), &["approve", STAGE_ID, STAGE_ID, "--otp", "123456", "--reporter=silent"]);

    read_mock.assert();
    approve_mock.assert();
    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("Staged package {STAGE_ID} approved and published successfully.\n"),
    );
}

#[test]
fn approve_without_a_stage_id_requires_an_interactive_terminal() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    let mock = server.mock("GET", Matcher::Any).expect(0).create();

    // The spawned binary has no TTY, so the staged packages cannot be chosen.
    let output = stage(dir.path(), &["approve"]);

    mock.assert();
    assert_failure_with_code(&output, "ERR_PNPM_STAGE_ID_REQUIRED");
}

#[test]
fn approve_rejects_a_stage_id_that_is_not_a_uuid() {
    let dir = tempfile::tempdir().expect("workspace");
    write_registry_config(dir.path(), "http://localhost:4873/");

    let output = stage(dir.path(), &["approve", STAGE_ID, "not-a-uuid"]);

    assert_failure_with_code(&output, "ERR_PNPM_INVALID_STAGE_ID");
}

#[test]
fn approve_maps_a_web_auth_challenge_to_the_non_interactive_error() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    server
        .mock("POST", format!("/-/stage/{STAGE_ID}/approve").as_str())
        .with_status(401)
        .with_body(
            json!({
                "authUrl": "https://www.npmjs.com/auth/cli/test-auth-id",
                "doneUrl": "https://registry.example.com/-/v1/done?authId=test-auth-id",
            })
            .to_string(),
        )
        .create();

    // The spawned binary has no TTY, so the web-auth challenge cannot be
    // driven interactively.
    let output = stage(dir.path(), &["approve", STAGE_ID]);

    assert_failure_with_code(&output, "ERR_PNPM_OTP_NON_INTERACTIVE");
}

#[test]
fn approve_surfaces_a_plain_401_as_a_stage_registry_error() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_registry_config(dir.path(), &registry);
    server
        .mock("POST", format!("/-/stage/{STAGE_ID}/approve").as_str())
        .with_status(401)
        .with_header("www-authenticate", r#"Basic realm="example""#)
        .with_body(r#"{"error":"unauthorized"}"#)
        .create();

    let output = stage(dir.path(), &["approve", STAGE_ID]);

    assert_failure_with_code(&output, "ERR_PNPM_STAGE_REGISTRY_ERROR");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("Failed to approve staged package {STAGE_ID}")),
        "stderr: {stderr}",
    );
    // miette wraps long lines, so the status clause is asserted separately.
    assert!(stderr.contains("(status 401 Unauthorized)"), "stderr: {stderr}");
}
