use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn star_unauthorized() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    fs::write(root.path().join(".npmrc"), "registry=http://127.0.0.1:9/").unwrap();

    let output = pacquet.with_arg("star").with_arg("foo").output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_STAR_UNAUTHORIZED"));
}

#[test]
fn stars_unauthorized() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    fs::write(root.path().join(".npmrc"), "registry=http://127.0.0.1:9/").unwrap();

    let output = pacquet.with_arg("stars").output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_STARS_UNAUTHORIZED"));
}

#[test]
fn unstar_unauthorized() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    fs::write(root.path().join(".npmrc"), "registry=http://127.0.0.1:9/").unwrap();

    let output = pacquet.with_arg("unstar").with_arg("foo").output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_STAR_UNAUTHORIZED"));
}
