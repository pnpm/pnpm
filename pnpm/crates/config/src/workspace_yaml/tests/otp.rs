use super::{
    WORKSPACE_MANIFEST_FILENAME,
    WorkspaceSettings,
    assert_eq,
    fs,
};

/// A one-time password lasts one invocation, so no file may carry it. The key
/// names a real setting, so it is reported as refused rather than
/// unrecognized, and the warning routes it to `--otp` / `PNPM_CONFIG_OTP`.
#[test]
fn a_workspace_manifest_refuses_otp() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(WORKSPACE_MANIFEST_FILENAME), "otp: \"123456\"\n").unwrap();

    let settings = WorkspaceSettings::load_at(dir.path())
        .expect("load pnpm-workspace.yaml")
        .expect("pnpm-workspace.yaml is present");

    assert_eq!(settings.key_issues.refused, ["otp"]);
    assert!(settings.key_issues.unrecognized.is_empty());
}
