mod http_upload;
mod process;
mod rollback;
mod store;
mod which;
mod metadata_file {
    include!(concat!(
        env!("PNPM_REPO_ROOT"),
        "/pnpm/crates/install-coordinator/src/metadata_file_wasi.rs"
    ));
}

use rayon::prelude::*;
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    thread,
};

#[tokio::main(worker_threads = 2)]
async fn main() {
    if std::env::var_os("PROBE_THREAD_EXIT").is_some() {
        thread::spawn(|| std::process::exit(23)).join().unwrap();
        unreachable!("process exit terminates every thread");
    }
    let directory = std::env::var("PROBE_DIR").expect("PROBE_DIR must name a writable directory");
    let directory = Path::new(&directory);
    if std::env::var_os("PROBE_THREADS_ONLY").is_some() {
        check_dynamic_thread_startup(directory);
        return;
    }
    if std::env::var_os("PROBE_PROCESS_ONLY").is_some() {
        process::check(directory).await;
        return;
    }
    if let Ok(mode) = std::env::var("PROBE_STORE") {
        store::check(directory, &mode);
        return;
    }
    store::check(directory, "roundtrip");
    rollback::check(directory);
    check_async_filesystem(directory).await;
    which::check(directory);
    check_parallel_work();
    check_dynamic_thread_startup(directory);
    check_shared_descriptors(directory);
    check_secure_filesystem(directory).await;
    check_host_network().await;
    check_http_client().await;
    http_upload::check().await;
    check_host_process().await;
    check_host_errors_and_cancellation().await;
    process::check(directory).await;
    println!("pnpm-wasm-runtime-probe-ok");
}

fn check_dynamic_thread_startup(directory: &Path) {
    let path = directory.join("pnpm-dynamic-thread-startup");
    std::fs::write(&path, b"shared-filesystem").unwrap();
    let barrier = std::sync::Barrier::new(13);
    thread::scope(|scope| {
        let threads: Vec<_> = (0..12)
            .map(|_| {
                scope.spawn(|| {
                    assert_eq!(std::fs::read(&path).unwrap(), b"shared-filesystem");
                    barrier.wait();
                })
            })
            .collect();
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
    });
    std::fs::remove_file(path).unwrap();
    println!("Twelve simultaneous guest threads completed WASI filesystem operations");
}

async fn check_http_client() {
    let url = std::env::var("PROBE_REGISTRY_URL")
        .unwrap_or_else(|_| "https://registry.npmjs.org/is-odd/latest".into());
    let client = pnpm_network::ThrottledClient::new_for_installs();
    let metadata: Value = client
        .acquire()
        .await
        .get(url)
        .send()
        .await
        .expect("HTTP facade request")
        .error_for_status()
        .expect("HTTP facade status")
        .json()
        .await
        .expect("HTTP facade JSON body");
    assert_eq!(metadata["name"], "is-odd");
    let error = pnpm_http::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect_err("TLS verification configuration must not be silently ignored");
    assert!(error.is_builder());
    println!("Shared pnpm network client and unsupported TLS policy rejection passed");
}

async fn host(message: Value) -> Value {
    pnpm_wasm_host::request(&message)
        .expect("start host operation")
        .await
        .expect("complete host operation")
}

async fn check_host_network() {
    let url = std::env::var("PROBE_REGISTRY_URL")
        .unwrap_or_else(|_| "https://registry.npmjs.org/is-odd/latest".into());
    let response = host(json!({ "operation": "network.request", "url": url })).await;
    assert_eq!(response["status"], 200);
    let handle = &response["handle"];
    let mut bytes = Vec::<u8>::new();
    loop {
        let chunk = host(json!({ "operation": "stream.read", "handle": handle })).await;
        if chunk["done"] == true {
            break;
        }
        bytes.extend(
            serde_json::from_value::<Vec<u8>>(chunk["bytes"].clone()).expect("binary chunk"),
        );
    }
    let metadata: Value = serde_json::from_slice(&bytes).expect("registry returned JSON");
    assert_eq!(metadata["name"], "is-odd");
    host(json!({ "operation": "resource.close", "handle": handle })).await;
    println!("Rust async host HTTP and streaming passed");
}

async fn check_host_process() {
    let child = host(json!({
        "operation": "process.spawn", "program": "node",
        "args": ["-e", "process.stdin.pipe(process.stdout); process.exitCode = 17"],
    }))
    .await;
    let handle = &child["handle"];
    host(json!({ "operation": "process.write", "handle": handle, "bytes": [0, 255, 13] })).await;
    host(json!({ "operation": "process.end", "handle": handle })).await;
    let output = host(json!({ "operation": "stream.read", "handle": child["stdout"] })).await;
    assert_eq!(output["bytes"], json!([0, 255, 13]));
    let exit = host(json!({ "operation": "process.wait", "handle": handle })).await;
    assert_eq!(exit["code"], 17);
    assert!(exit["signal"].is_null());
    for handle in [&child["stdout"], &child["stderr"], handle] {
        host(json!({ "operation": "resource.close", "handle": handle })).await;
    }
    println!("Rust async host subprocess and binary pipes passed");
}

async fn check_host_errors_and_cancellation() {
    let error = pnpm_wasm_host::request(&json!({
        "operation": "process.spawn", "program": "/pnpm-wasm-missing-executable",
    }))
    .expect("start failing operation")
    .await
    .expect_err("missing executable must fail");
    assert_eq!(error.code.as_deref(), Some("ENOENT"));
    let cancelled = pnpm_wasm_host::request(&json!({ "operation": "invalid-probe-operation" }))
        .expect("start cancellable operation");
    drop(cancelled);
    let error = pnpm_wasm_host::request(&json!({ "operation": "stream.read", "handle": 0 }))
        .expect("start operation after cancellation")
        .await
        .expect_err("invalid stream must fail");
    assert!(error.message.contains("Invalid stream handle"));
    println!("Rust host errors and dropped request cancellation passed");
}

async fn check_async_filesystem(directory: &Path) {
    let path = directory.join("tokio-filesystem");
    tokio::fs::write(&path, b"tokio-filesystem").await.expect("write through tokio");
    let contents = tokio::fs::read(&path).await.expect("read through tokio");
    assert_eq!(contents, b"tokio-filesystem");
    tokio::fs::remove_file(&path).await.expect("remove through tokio");
    tokio::spawn(async { tokio::time::sleep(std::time::Duration::from_millis(1)).await })
        .await
        .expect("join async task");
    println!("Tokio filesystem, task scheduling and timers passed");
}

fn check_parallel_work() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .expect("build rayon pool");
    let total = pool.install(|| (0..100usize).into_par_iter().sum::<usize>());
    assert_eq!(total, 4950);
    println!("Rayon parallel work passed");
}

fn check_shared_descriptors(directory: &Path) {
    let path = directory.join("shared-descriptor");
    let mut writer = File::create(&path).expect("create descriptor on main thread");
    thread::spawn(move || writer.write_all(b"shared-descriptor"))
        .join()
        .expect("join writer")
        .expect("write main-thread descriptor from another thread");
    let reader_path = path.clone();
    let mut reader = thread::spawn(move || File::open(reader_path))
        .join()
        .expect("join opener")
        .expect("open descriptor on another thread");
    let mut contents = String::new();
    reader.read_to_string(&mut contents).expect("read worker descriptor on main thread");
    assert_eq!(contents, "shared-descriptor");
    drop(reader);
    std::fs::remove_file(&path).expect("remove shared-descriptor file");
    println!("File descriptors shared between threads passed");
}

async fn check_secure_filesystem(directory: &Path) {
    let temporary = pnpm_fs::private_named_tempfile_in(directory).expect("private tempfile");
    assert_eq!(pnpm_fs::read_file_permissions(temporary.as_file()).unwrap() & 0o777, 0o600);
    drop(temporary);
    let credential = directory.join("credential");
    pnpm_fs::write_atomic_private(&credential, b"secret").expect("private atomic write");
    let executable = directory.join("executable");
    pnpm_fs::write_atomic(&executable, b"script").expect("atomic write");
    pnpm_fs::file_mode::set_path_permissions(&executable, 0o755).expect("executable permissions");
    pnpm_fs::executable_access(&executable).expect("host executable access");
    assert!(pnpm_fs::executable_access(&credential).is_err());
    let linked = directory.join("linked");
    pnpm_fs::create_symlink(&credential, &linked, false).expect("WASI symlink");
    assert_eq!(std::fs::read(&linked).expect("read symlink"), b"secret");
    assert!(pnpm_fs::file_mode::set_path_permissions(&linked, 0o777).is_err());
    std::fs::remove_file(&linked).expect("remove symlink");
    std::fs::hard_link(&credential, &linked).expect("hardlink");
    assert_eq!(pnpm_fs::file_link_count(&File::open(&credential).unwrap()).unwrap(), 2);
    assert!(pnpm_fs::overwrite_file_in_place(&credential, &mut &b"repaired"[..]));
    assert_eq!(std::fs::read(&linked).expect("read healed hardlink"), b"repaired");
    let copied = directory.join("copied-credential");
    pnpm_fs::copy_file_atomic(&credential, &copied).expect("copy private file");
    assert_eq!(pnpm_fs::copy_permissions(&copied).unwrap() & 0o777, 0o600);
    std::fs::remove_file(copied).unwrap();
    check_host_file_modes(&credential, &executable).await;
    for file in [&credential, &executable, &linked] {
        std::fs::remove_file(file).expect("remove filesystem fixture");
    }
    println!("pnpm filesystem secure creation, permissions, symlinks and hardlink repair passed");
}

async fn check_host_file_modes(credential: &Path, executable: &Path) {
    let script = "const fs=require('node:fs'); const modes=process.argv.slice(1).map(p=>fs.statSync(p).mode&0o777); if(modes[0]!==0o600||modes[1]!==0o755) throw new Error(JSON.stringify(modes))";
    let child = host(json!({
        "operation": "process.spawn", "program": "node",
        "args": ["-e", script, credential, executable],
        "stdin": "ignore", "stdout": "inherit", "stderr": "inherit",
    }))
    .await;
    let exit = host(json!({ "operation": "process.wait", "handle": child["handle"] })).await;
    assert_eq!(exit["code"], 0);
    host(json!({ "operation": "resource.close", "handle": child["handle"] })).await;
}
