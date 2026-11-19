use std::fs;

use mockito::Matcher;
use pnpm_testing_utils::diagnostics::assert_diagnostic_contains;
use serde_json::json;

use super::{publish, write_project};

#[test]
fn publish_rejects_publish_config_directory_escaping_project_root() {
    let outer = tempfile::tempdir().expect("outer");
    let outside = outer.path().join("outside");
    let pkg_dir = outer.path().join("pkg");
    fs::create_dir_all(&outside).expect("create outside");
    fs::create_dir_all(&pkg_dir).expect("create pkg");
    fs::write(
        outside.join("package.json"),
        json!({ "name": "outside-pkg", "version": "1.0.0" }).to_string(),
    )
    .expect("write outside package.json");
    fs::write(outside.join("secret.txt"), "SENSITIVE_DATA=123\n").expect("write secret.txt");

    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    let unexpected = server
        .mock("PUT", Matcher::Any)
        .expect(0)
        .create();

    let outside_str = outside
        .to_str()
        .expect("utf8 outside")
        .to_string();
    for directory in [outside_str.as_str(), "../outside", "/dist"] {
        write_project(
            &pkg_dir,
            &registry,
            &json!({
                "name": "test-publish-escape",
                "version": "1.0.0",
                "publishConfig": { "directory": directory },
            }),
        );

        let output = publish(&pkg_dir, &["--ignore-scripts"]);
        assert!(!output.status.success(), "directory {directory:?} must fail");
        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert_diagnostic_contains(&combined, "ERR_PNPM_INVALID_PUBLISH_DIRECTORY");
    }

    unexpected.assert();
}
