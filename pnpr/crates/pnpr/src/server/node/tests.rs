use super::artifact_basename;

#[test]
fn release_artifacts_require_the_requested_version_and_a_supported_path() {
    let version = "v22.0.0";
    for path in [
        "node-v22.0.0-headers.tar.gz",
        "node-v22.0.0-win-x64.zip",
        "node-v22.0.0-linux-arm64.tar.xz",
    ] {
        assert_eq!(artifact_basename(version, path), Some(path));
    }
    for path in [
        "node-v22.0.1-headers.tar.gz",
        "node-v22.0.0-linux/x64.zip",
        "node-v22.0.0-.zip",
        "node-v22.0.0-win-x64.exe",
        "win-x64/other.lib",
    ] {
        assert_eq!(artifact_basename(version, path), None);
    }
}
