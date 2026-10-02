use super::{FsWrite, Host};
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn atomic_replacement_publishes_an_executable_shim() {
    let directory = tempfile::tempdir().unwrap();
    let shim = directory.path().join("executable shim");

    for message in ["created", "replaced"] {
        let source = format!("#!/bin/sh\nprintf '{message}\\n'\n");
        Host::write_replace(&shim, source.as_bytes()).unwrap();

        assert_eq!(
            fs::metadata(&shim)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755,
        );
        let output = Command::new(&shim).output().unwrap();
        assert!(output.status.success(), "shim failed: {output:?}");
        assert_eq!(output.stdout, format!("{message}\n").as_bytes());
    }
}
