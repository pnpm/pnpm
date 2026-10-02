use std::{ffi::OsStr, fs, path::Path};

pub fn check(directory: &Path) {
    let root = directory.join("which-probe");
    let blocked = root.join("blocked");
    let executable = root.join("executable");
    fs::create_dir_all(&blocked).unwrap();
    fs::create_dir_all(&executable).unwrap();
    drop(pnpm_fs::create_new_with_mode(&blocked.join("tool"), 0o600).unwrap());
    drop(pnpm_fs::create_new_with_mode(&executable.join("tool"), 0o700).unwrap());
    let paths = pnpm_fs::join_paths([&blocked, &executable]).unwrap();
    assert_eq!(pnpm_which::which_in("tool", Some(&paths), &root).unwrap(), executable.join("tool"));
    assert_eq!(
        pnpm_which::which_in_global("tool", Some(&paths)).unwrap().collect::<Vec<_>>(),
        vec![executable.join("tool")]
    );
    assert!(pnpm_which::which_in("tool", None::<&OsStr>, &root).is_err());
    assert_eq!(
        pnpm_which::which_in("./tool", None::<&OsStr>, &executable).unwrap(),
        executable.join("tool")
    );
    assert!(pnpm_which::which("node").is_ok());
    fs::remove_dir_all(root).unwrap();
    println!("Executable lookup preserves PATH ordering and host executable permissions");
}
