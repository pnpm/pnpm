use super::ProvisionedEngine;
use std::{fs, path::PathBuf};

/// A bin named `name` in `bin_dir` that `which` accepts on this platform.
fn write_bin(bin_dir: &std::path::Path, name: &str) -> PathBuf {
    let file = bin_dir.join(if cfg!(windows) { format!("{name}.cmd") } else { name.to_string() });
    fs::write(&file, "").expect("write bin");
    let opened = fs::File::open(&file).expect("open bin");
    pnpm_fs::file_mode::make_file_executable(&opened).expect("make bin executable");
    file
}

#[test]
fn the_main_command_runs_the_native_program_instead_of_its_bin() {
    let root = tempfile::TempDir::new().expect("tmp dir");
    let bin_dir = root.path().join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin dir");
    write_bin(&bin_dir, "pnpm");
    let pnpx = write_bin(&bin_dir, "pnpx");
    let program = root
        .path()
        .join("wrapper")
        .join(if cfg!(windows) { "pnpm.exe" } else { "pnpm" });
    let engine = ProvisionedEngine {
        program: program.clone(),
        bin_dirs: vec![bin_dir],
        _private_installs: vec![],
    };

    assert_eq!(engine.command("pnpm"), program);
    assert_eq!(engine.command("pnpx"), pnpx);
    assert_eq!(engine.command("pn"), program, "a command the engine lacks runs its main program");
}
