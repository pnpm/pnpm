use super::output_retrying_busy;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::Command,
    thread,
    time::Duration,
};

fn write_executable(path: &std::path::Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The shim's own `exec` of `node` fails while the test holds `node` open
/// for writing. A translated locale must not hide that diagnostic.
#[test]
fn retries_a_shim_whose_exec_of_a_busy_file_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let node = tmp.path().join("node");
    write_executable(&node, "#!/bin/sh\necho ok\n");
    let shim = tmp.path().join("shim");
    write_executable(&shim, &format!("#!/bin/sh\nexec '{}'\n", node.display()));
    let writer = fs::OpenOptions::new()
        .append(true)
        .open(&node)
        .unwrap();
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        drop(writer);
    });

    let output = output_retrying_busy(Command::new(&shim).env("LC_ALL", "de_DE.UTF-8"));

    release.join().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"ok\n");
}
