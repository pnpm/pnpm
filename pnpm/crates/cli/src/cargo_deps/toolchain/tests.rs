use super::program;
use std::{fs, path::PathBuf};

#[test]
fn a_toolchain_the_checkout_committed_is_not_run() {
    let checkout = tempfile::tempdir().unwrap();
    let checkout = dunce::canonicalize(checkout.path()).unwrap();
    fs::write(checkout.join("rust-toolchain.toml"), "[toolchain]\npath = \"/opt/rust\"\n").unwrap();
    let committed = checkout.join(".pnpm/rust/bin");
    fs::create_dir_all(&committed).unwrap();
    fs::write(committed.join(format!("cargo{}", std::env::consts::EXE_SUFFIX)), "").unwrap();

    assert_eq!(program("cargo", &checkout, Some(&checkout)), PathBuf::from("cargo"));
}
