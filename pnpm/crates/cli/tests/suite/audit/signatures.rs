use super::{
    CommandTempCwd, Path, assert_failure, assert_success, fs, stderr, stdout,
    write_minimal_manifest, write_two_project_audit_workspace,
};
use pnpm_testing_utils::trusted_tls_server::TrustedTlsServer;

const SIGNATURE_KEYID: &str = "SHA256:test";

#[test]
fn audit_signatures_reports_verified_packages() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let key = signing_key();
    let integrity = "sha512-YWJj";
    let signature = sign_b64(&key, &format!("signed-pkg@1.0.0:{integrity}"));
    let keys_mock = keys_mock(&mut registry, &public_key_b64(&key)).create();
    let packument_mock = registry
        .mock("GET", "/signed-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body("signed-pkg", "1.0.0", integrity, &signatures_json(&signature)))
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .output()
        .expect("run audit signatures");

    assert_success(&output);
    let out = stdout(&output);
    assert!(out.contains("audited 1 package"), "{out}");
    assert!(out.contains("1 package has a verified registry signature"), "{out}");
    keys_mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_signatures_cannot_cover_missing_or_different_lockfile_integrity() {
    for resolution in
        ["{tarball: https://registry.example/signed-pkg.tgz}", "{integrity: sha512-ZGVm}", "env"]
    {
        let CommandTempCwd {
            mut pacquet, workspace, root: _root, ..
        } = CommandTempCwd::init();
        let mut registry = mockito::Server::new();
        let key = signing_key();
        let integrity = "sha512-YWJj";
        let signature = sign_b64(&key, &format!("signed-pkg@1.0.0:{integrity}"));
        let keys_mock = keys_mock(&mut registry, &public_key_b64(&key)).create();
        let packument_mock = registry
            .mock("GET", "/signed-pkg")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(packument_body(
                "signed-pkg",
                "1.0.0",
                integrity,
                &signatures_json(&signature),
            ))
            .create();
        write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");
        let lockfile_path = workspace.join("pnpm-lock.yaml");
        let original = fs::read_to_string(&lockfile_path).unwrap();
        let lockfile = if resolution == "env" {
            format!(
                "---\nlockfileVersion: '9.0'\nimporters:\n  .:\n    configDependencies:\n      signed-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  signed-pkg@1.0.0:\n    resolution: {{integrity: sha512-ZGVm}}\nsnapshots:\n  signed-pkg@1.0.0: {{}}\n---\n{original}",
            )
        } else {
            original.replace("{integrity: sha512-YWJj}", resolution)
        };
        fs::write(lockfile_path, lockfile).unwrap();
        let output = pacquet
            .args(["audit", "signatures", "--json"])
            .output()
            .unwrap();
        assert_failure(&output);
        let result: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
        assert_eq!(result["verified"], i32::from(resolution == "env"));
        assert_eq!(
            result["invalid"]
                .as_array()
                .unwrap()
                .len(),
            1,
        );
        keys_mock.assert();
        packument_mock.assert();
    }
}

#[test]
fn audit_signatures_filter_checks_only_the_selected_projects() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let key = signing_key();
    let integrity = "sha512-YWJj";
    let signature = sign_b64(&key, &format!("minimist@1.2.0:{integrity}"));
    let keys_mock = keys_mock(&mut registry, &public_key_b64(&key)).create();
    let selected_mock = registry
        .mock("GET", "/minimist")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body("minimist", "1.2.0", integrity, &signatures_json(&signature)))
        .expect(1)
        .create();
    let unselected_mock = registry
        .mock("GET", "/lodash")
        .expect(0)
        .create();
    write_two_project_audit_workspace(&workspace, &registry.url());

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .arg("--filter")
        .arg("project-b")
        .output()
        .expect("run audit signatures");

    assert_success(&output);
    assert!(stdout(&output).contains("audited 1 package"), "{}", stdout(&output));
    keys_mock.assert();
    selected_mock.assert();
    unselected_mock.assert();
}

#[test]
fn audit_signatures_json_reports_counts() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let key = signing_key();
    let integrity = "sha512-YWJj";
    let signature = sign_b64(&key, &format!("signed-pkg@1.0.0:{integrity}"));
    let keys_mock = keys_mock(&mut registry, &public_key_b64(&key)).create();
    let packument_mock = registry
        .mock("GET", "/signed-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body("signed-pkg", "1.0.0", integrity, &signatures_json(&signature)))
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .arg("--json")
        .output()
        .expect("run audit signatures");

    assert_success(&output);
    let out = stdout(&output);
    assert!(out.ends_with('\n'), "signatures JSON should end with a newline:\n{out}");
    let report: serde_json::Value = serde_json::from_str(&out).expect("signatures JSON");
    assert_eq!(report["audited"], 1);
    assert_eq!(report["verified"], 1);
    assert_eq!(
        report["invalid"]
            .as_array()
            .expect("invalid array")
            .len(),
        0,
    );
    assert_eq!(
        report["missing"]
            .as_array()
            .expect("missing array")
            .len(),
        0,
    );
    keys_mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_signatures_flags_missing_signature() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let key = signing_key();
    let keys_mock = keys_mock(&mut registry, &public_key_b64(&key)).create();
    let packument_mock = registry
        .mock("GET", "/signed-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body("signed-pkg", "1.0.0", "sha512-YWJj", "[]"))
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .output()
        .expect("run audit signatures");

    assert_eq!(output.status.code(), Some(1), "missing signatures should exit 1");
    let out = stdout(&output);
    assert!(out.contains("missing registry signature"), "{out}");
    assert!(out.contains("signed-pkg@1.0.0"), "{out}");
    keys_mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_signatures_flags_invalid_signature() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let key = signing_key();
    // Sign a different integrity than the packument advertises: the signature
    // is well-formed but will not validate over the published bytes.
    let signature = sign_b64(&key, "signed-pkg@1.0.0:sha512-tampered");
    let keys_mock = keys_mock(&mut registry, &public_key_b64(&key)).create();
    let packument_mock = registry
        .mock("GET", "/signed-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body(
            "signed-pkg",
            "1.0.0",
            "sha512-YWJj",
            &signatures_json(&signature),
        ))
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .output()
        .expect("run audit signatures");

    assert_eq!(output.status.code(), Some(1), "invalid signatures should exit 1");
    let out = stdout(&output);
    assert!(out.contains("invalid registry signature"), "{out}");
    assert!(out.contains("Someone might have tampered"), "{out}");
    keys_mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_signatures_skips_registry_without_signing_keys() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let keys_mock = registry
        .mock("GET", "/-/npm/v1/keys")
        .with_status(404)
        .with_body("not found")
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .output()
        .expect("run audit signatures");

    assert_success(&output);
    let out = stdout(&output);
    assert!(out.contains("audited 0 packages"), "{out}");
    assert!(
        out.contains("No dependencies were installed from a registry with signing keys"),
        "{out}",
    );
    keys_mock.assert();
}

#[test]
fn audit_signatures_fails_when_keys_endpoint_errors() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let keys_mock = registry
        .mock("GET", "/-/npm/v1/keys")
        .with_status(500)
        .with_body("boom \u{1b}[31m\n")
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .output()
        .expect("run audit signatures");

    assert_failure(&output);
    let stderr = stderr(&output);
    assert!(stderr.contains("ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL"), "stderr:\n{stderr}");
    assert!(stderr.contains("responded with 500"), "stderr:\n{stderr}");
    // The attacker-controlled registry body is escaped before it reaches the
    // terminal: the raw ESC byte must not survive.
    assert!(stderr.contains(r"boom \u{1b}[31m\u{a}"), "stderr:\n{stderr}");
    assert!(!stderr.contains('\u{1b}'), "stderr:\n{stderr}");
    keys_mock.assert();
}

#[test]
fn audit_signatures_fails_when_keys_response_is_too_large() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let keys_mock = registry
        .mock("GET", "/-/npm/v1/keys")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(" ".repeat(2 * 1024 * 1024))
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .output()
        .expect("run audit signatures");

    assert_failure(&output);
    let stderr = stderr(&output);
    assert!(stderr.contains("ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL"), "stderr:\n{stderr}");
    assert!(stderr.contains("larger than 1048576 bytes"), "stderr:\n{stderr}");
    keys_mock.assert();
}

#[test]
fn audit_signatures_follows_a_keys_redirect_with_the_target_tls_settings() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    let key = signing_key();
    let integrity = "sha512-YWJj";
    let signature = sign_b64(&key, &format!("signed-pkg@1.0.0:{integrity}"));
    let keys_server = TrustedTlsServer::start(&keys_body(&public_key_b64(&key)));
    let mut registry = mockito::Server::new();
    let keys_mock = registry
        .mock("GET", "/-/npm/v1/keys")
        .with_status(302)
        .with_header("location", &format!("{}/-/npm/v1/keys", keys_server.url))
        .create();
    let packument_mock = registry
        .mock("GET", "/signed-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(packument_body("signed-pkg", "1.0.0", integrity, &signatures_json(&signature)))
        .create();
    write_signatures_workspace(&workspace, &registry.url(), "signed-pkg");
    // The CA scoped to the registry does not trust the redirect target.
    fs::write(
        workspace.join(".npmrc"),
        format!(
            "registry={}/\ncafile={}\n//{}/:cafile={}\n",
            registry.url(),
            TrustedTlsServer::ca_path().display(),
            registry.host_with_port(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../network/tests/fixtures/test-ca.pem")
                .display(),
        ),
    )
    .expect("write .npmrc");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .output()
        .expect("run audit signatures");

    assert_success(&output);
    let out = stdout(&output);
    assert!(out.contains("1 package has a verified registry signature"), "{out}");
    keys_mock.assert();
    packument_mock.assert();
}

#[test]
fn audit_signatures_redacts_registry_credentials_on_network_error() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    // A registry with embedded credentials pointed at an address whose connect
    // fails at once: the keys fetch fails at the transport layer, and the
    // resulting error must not leak the `user:pass@` userinfo into stderr.
    write_signatures_workspace(&workspace, "https://user:pass@0.0.0.0:1", "signed-pkg");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .output()
        .expect("run audit signatures");

    assert_failure(&output);
    let stderr = stderr(&output);
    assert!(stderr.contains("ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL"), "stderr:\n{stderr}");
    assert!(!stderr.contains("user:pass"), "credentials leaked into stderr:\n{stderr}");
    assert!(!stderr.contains("pass@"), "credentials leaked into stderr:\n{stderr}");
}

#[test]
fn audit_signatures_errors_when_no_packages() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    fs::write(workspace.join(".npmrc"), "registry=https://registry.npmjs.org/\n")
        .expect("write .npmrc");
    fs::write(workspace.join("pnpm-workspace.yaml"), "fetchRetries: 0\n")
        .expect("write workspace manifest");
    write_minimal_manifest(&workspace);
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        "
lockfileVersion: '9.0'

importers:

  .: {}
",
    )
    .expect("write lockfile");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .output()
        .expect("run audit signatures");

    assert_failure(&output);
    assert!(stderr(&output).contains("ERR_PNPM_AUDIT_NO_PACKAGES"), "stderr:\n{}", stderr(&output));
}
#[test]
fn audit_signatures_errors_when_lockfile_has_unresolved_dependency() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    fs::write(workspace.join(".npmrc"), "registry=https://registry.npmjs.org/\n")
        .expect("write .npmrc");
    write_minimal_manifest(&workspace);
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      missing-pkg:
        specifier: 1.0.0
        version: 1.0.0

packages: {}

snapshots: {}
",
    )
    .expect("write lockfile");

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .output()
        .expect("run audit signatures");

    assert_failure(&output);
    assert!(
        stderr(&output).contains("ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY"),
        "stderr:\n{}",
        stderr(&output),
    );
}

#[test]
fn audit_signatures_rejects_extra_subcommand_argument() {
    let CommandTempCwd {
        mut pacquet, workspace, root: _root, ..
    } = CommandTempCwd::init();
    write_minimal_manifest(&workspace);

    let output = pacquet
        .arg("audit")
        .arg("signatures")
        .arg("extra")
        .output()
        .expect("run pacquet audit");

    assert_failure(&output);
    assert!(stderr(&output).contains("ERR_PNPM_AUDIT_UNKNOWN_SUBCOMMAND"));
    assert!(stderr(&output).contains("Unknown audit subcommand: signatures extra"));
}

fn signing_key() -> p256::ecdsa::SigningKey {
    p256::ecdsa::SigningKey::from_slice(&[0x42; 32]).expect("valid P-256 scalar")
}

fn public_key_b64(key: &p256::ecdsa::SigningKey) -> String {
    use base64::Engine as _;
    use p256::pkcs8::EncodePublicKey;
    let der = key
        .verifying_key()
        .to_public_key_der()
        .expect("encode SPKI");
    base64::engine::general_purpose::STANDARD.encode(der.as_bytes())
}

fn sign_b64(key: &p256::ecdsa::SigningKey, message: &str) -> String {
    use base64::Engine as _;
    use p256::ecdsa::{Signature, signature::Signer};
    let signature: Signature = key.sign(message.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(signature.to_der().as_bytes())
}

fn keys_mock(registry: &mut mockito::Server, public_key_b64: &str) -> mockito::Mock {
    registry
        .mock("GET", "/-/npm/v1/keys")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(keys_body(public_key_b64))
}

fn keys_body(public_key_b64: &str) -> String {
    format!(
        r#"{{"keys":[{{"expires":null,"keyid":"{SIGNATURE_KEYID}","keytype":"ecdsa-sha2-nistp256","scheme":"ecdsa-sha2-nistp256","key":"{public_key_b64}"}}]}}"#,
    )
}

fn signatures_json(signature_b64: &str) -> String {
    format!(r#"[{{"keyid":"{SIGNATURE_KEYID}","sig":"{signature_b64}"}}]"#)
}

fn packument_body(name: &str, version: &str, integrity: &str, signatures_json: &str) -> String {
    format!(
        r#"{{"name":"{name}","versions":{{"{version}":{{"name":"{name}","version":"{version}","dist":{{"integrity":"{integrity}","tarball":"https://example.com/{name}-{version}.tgz","signatures":{signatures_json}}}}}}},"time":{{"{version}":"2020-01-01T00:00:00.000Z"}}}}"#,
    )
}

fn write_signatures_workspace(workspace: &Path, registry_url: &str, name: &str) {
    fs::write(workspace.join(".npmrc"), format!("registry={registry_url}/\n"))
        .expect("write .npmrc");
    fs::write(workspace.join("pnpm-workspace.yaml"), "fetchRetries: 0\n")
        .expect("write workspace manifest");
    fs::write(
        workspace.join("package.json"),
        format!(r#"{{"name":"sig-test","version":"1.0.0","dependencies":{{"{name}":"1.0.0"}}}}"#),
    )
    .expect("write package.json");
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        format!(
            "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      {name}:
        specifier: '1.0.0'
        version: '1.0.0'

packages:

  {name}@1.0.0:
    resolution: {{integrity: sha512-YWJj}}

snapshots:

  {name}@1.0.0: {{}}
",
        ),
    )
    .expect("write lockfile");
}
