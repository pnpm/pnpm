use std::{fs, path::Path, process::Command, thread, time::SystemTime};

fn main() {
    let directory = std::env::var("PROBE_DIR").expect("PROBE_DIR must name a WASI preopen");
    let directory = Path::new(&directory);
    fs::write(directory.join("input"), b"pnpm-wasm-probe").unwrap();
    fs::rename(directory.join("input"), directory.join("output")).unwrap();
    assert_eq!(fs::read(directory.join("output")).unwrap(), b"pnpm-wasm-probe");
    fs::hard_link(directory.join("output"), directory.join("linked")).unwrap();
    assert_eq!(fs::read(directory.join("linked")).unwrap(), b"pnpm-wasm-probe");
    println!("WASI clock: {:?}", SystemTime::now());
    println!("WASI threads: {:?}", thread::Builder::new().spawn(|| 42).map(|task| task.join()));
    println!("WASI processes: {:?}", Command::new("node").arg("--version").output());
    println!("pnpm-wasm-probe-ok");
}
