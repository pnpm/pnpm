use pnpm_process::{Command, Stdio, asynchronous};
use std::{fs::File, path::Path};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn check(directory: &Path) {
    check_blocking(directory);
    check_working_directory(directory);
    check_shell(directory);
    check_async().await;
    check_stdin_drop().await;
    println!(
        "Shared process facade: output, stdin, file redirection, environment, errors and exit status passed"
    );
}

fn check_working_directory(directory: &Path) {
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(directory).unwrap();
    let output = Command::new("node")
        .args(["-e", "process.stdout.write(process.cwd())"])
        .current_dir(".")
        .output();
    std::env::set_current_dir(original).unwrap();
    let output = output.expect("inherit guest working directory");
    assert!(output.status.success());
    assert_eq!(Path::new(std::str::from_utf8(&output.stdout).unwrap()), directory);
    assert_eq!(
        pnpm_process::id(),
        std::env::var("PNPM_WASM_PID")
            .unwrap()
            .parse::<u32>()
            .unwrap()
    );
    assert_eq!(
        pnpm_fs::temp_dir(),
        std::path::PathBuf::from(std::env::var_os("PNPM_WASM_TMPDIR").unwrap())
    );
}

fn check_blocking(directory: &Path) {
    let output = Command::new("node")
        .args(["-e", "process.stdout.write(process.env.PNPM_PROCESS_TEST);process.stderr.write('stderr');process.exitCode=17"])
        .env("PNPM_PROCESS_TEST", "stdout")
        .current_dir(directory)
        .output().expect("capture child output");
    assert_eq!(output.stdout, b"stdout", "{output:?}");
    assert_eq!(output.stderr, b"stderr");
    assert_eq!(output.status.code(), Some(17));
    let redirected = directory.join("process-redirect");
    let status = Command::new("node")
        .args(["-e", "process.stdout.write('redirected')"])
        .stdout(File::create(&redirected).expect("create redirected output"))
        .status()
        .expect("redirect output to guest file");
    assert!(status.success());
    assert_eq!(std::fs::read(&redirected).unwrap(), b"redirected");
    std::fs::remove_file(redirected).unwrap();
    assert_eq!(
        Command::new("/nonexistent-pnpm-command")
            .status()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );
}

async fn check_async() {
    let mut child = asynchronous::Command::new("node")
        .args(["-e", "process.stdin.pipe(process.stdout)"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn asynchronous child");
    let mut input = child.stdin.take().unwrap();
    input.write_all(b"pipe-input").await.expect("write child stdin");
    input.shutdown().await.expect("close child stdin");
    drop(input);
    let mut output = String::new();
    child.stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .await
        .expect("read child stdout");
    assert_eq!(output, "pipe-input");
    assert!(
        child
            .wait()
            .await
            .expect("wait for child")
            .success()
    );
    let mut child = asynchronous::Command::new("node")
        .args(["-e", "setInterval(()=>{},1000)"])
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    assert_eq!(child.try_wait().unwrap(), None);
    child.kill().await.expect("kill child");
    let status = child.wait().await.unwrap();
    assert!(!status.success());
    assert_eq!(status.signal(), Some(9));
}

fn check_shell(directory: &Path) {
    let output = Command::shell_emulator(
        "echo \"$PNPM_SHELL_TEST\" | node -e \"process.stdin.pipe(process.stdout)\"",
    )
    .env("PNPM_SHELL_TEST", "shell-output")
    .current_dir(directory)
    .output()
    .expect("execute portable shell pipeline");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"shell-output\n");
    assert_eq!(
        Command::shell_emulator("echo 'unterminated")
            .status()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
}

async fn check_stdin_drop() {
    let mut child = asynchronous::Command::new("node")
        .args(["-e", "process.stdin.pipe(process.stdout)"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(b"queued-before-drop").await.unwrap();
    drop(input);
    assert_eq!(child.wait_with_output().await.unwrap().stdout, b"queued-before-drop");

    let mut child = asynchronous::Command::new("node")
        .args(["-e", "setInterval(()=>{},1000)"])
        .stdin(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let payload = vec![b'x'; 2 * 1024 * 1024];
    let _ = tokio::time::timeout(std::time::Duration::from_millis(100), input.write_all(&payload))
        .await;
    drop(input);
    child.kill().await.unwrap();
}
