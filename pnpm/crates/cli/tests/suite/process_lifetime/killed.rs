//! pnpm killed while its command runs, the first place pnpm differs from
//! the command run without it: the Job Object ends everything pnpm started,
//! detached processes included. Unix covers its own form of this in
//! `interrupt`, where a killed pnpm's watchdog ends the script's process
//! group.
#![cfg(windows)]

use super::node_launching_pacquet;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

/// A command that starts a detached child connected to `port`, then keeps
/// running once the child is connected, so the test can kill pnpm while the
/// command is still running. It exits 42 if the child never connects.
pub(super) fn make_connected_detached_node_script(ready_path: &Path, port: u16) -> String {
    let ready_path_json =
        serde_json::to_string(&ready_path.to_string_lossy()).expect("quote ready path");
    let detached_script = format!(
        "const fs = require('fs'); const socket = require('net').createConnection({{ host: '127.0.0.1', port: {port} }}, () => fs.writeFileSync({ready_path_json}, 'ready')); socket.on('data', () => process.exit(0))",
    );
    let detached_script_json = serde_json::to_string(&detached_script).expect("quote script");
    format!(
        "const {{ spawn }} = require('child_process'); const fs = require('fs'); const child = spawn(process.execPath, ['-e', {detached_script_json}], {{ detached: true, stdio: 'ignore' }}); child.unref(); const deadline = Date.now() + 10000; const wait = () => fs.existsSync({ready_path_json}) ? setInterval(() => {{}}, 1000) : Date.now() < deadline ? setTimeout(wait, 10) : process.exit(42); wait()",
    )
}

pub(super) fn assert_connection_closes(mut connection: std::net::TcpStream) {
    use std::io::{ErrorKind, Read, Write};

    // A socket accepted from a non-blocking listener inherits that mode on
    // Windows, which makes the read below return `WouldBlock` at once and
    // the timeout moot — the assertion would race the job object's cleanup
    // rather than wait for it.
    connection.set_nonblocking(false).expect("set detached child connection blocking");
    connection
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set detached child connection timeout");
    let mut byte = [0];
    let process_exited = match connection.read(&mut byte) {
        Ok(0) => true,
        Ok(_) => false,
        Err(err)
            if matches!(
                err.kind(),
                ErrorKind::BrokenPipe
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::ConnectionReset
                    | ErrorKind::NotConnected,
            ) =>
        {
            true
        }
        Err(err) if matches!(err.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => false,
        Err(err) => panic!("read detached child connection: {err}"),
    };
    if !process_exited {
        let _ = connection.write_all(b"exit");
        panic!("the detached process should be cleaned up");
    }
}

/// Wait for the detached fixture to connect, terminating `parent` if it never
/// does so a failed test does not leave the chain running.
pub(super) fn accept_detached_connection(
    listener: &std::net::TcpListener,
    parent: &mut std::process::Child,
) -> std::net::TcpStream {
    use std::io::ErrorKind;

    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        match listener.accept() {
            Ok((connection, _)) => return connection,
            Err(err) if err.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                let _ = parent.kill();
                let _ = parent.wait();
                panic!("the detached process did not connect before the deadline");
            }
            Err(err) => panic!("accept detached child connection: {err}"),
        }
    }
}

/// pnpm killed while its command runs is the one case where pnpm, rather
/// than the command, decides what happens to the command's process tree:
/// the Job Object ends all of it, detached processes included.
#[test]
fn exec_ends_a_detached_process_when_pnpm_is_killed() {
    use std::net::TcpListener;

    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let ready_path = workspace.join("detached-ready.txt");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listen for detached child");
    listener.set_nonblocking(true).expect("set listener nonblocking");
    let port = listener
        .local_addr()
        .expect("read listener address")
        .port();

    let mut pacquet_process = pacquet
        .with_arg("exec")
        .with_arg(super::node_executable())
        .with_arg("-e")
        .with_arg(make_connected_detached_node_script(&ready_path, port))
        .spawn()
        .expect("spawn pacquet exec");

    let connection = accept_detached_connection(&listener, &mut pacquet_process);
    pacquet_process.kill().expect("kill pacquet");
    let _ = pacquet_process.wait();
    assert_connection_closes(connection);

    drop(root);
}

/// Killing the Node process that launched pnpm makes libuv's job end pnpm,
/// and pnpm's own job then ends the command's tree, as `nr` users see when
/// they close what started it.
#[test]
fn exec_ends_a_detached_process_when_the_node_launching_pnpm_is_killed() {
    use std::net::TcpListener;

    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let ready_path = workspace.join("detached-ready.txt");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listen for detached child");
    listener.set_nonblocking(true).expect("set listener nonblocking");
    let port = listener
        .local_addr()
        .expect("read listener address")
        .port();
    let detached_script = make_connected_detached_node_script(&ready_path, port);

    let mut node_process = node_launching_pacquet(
        &pacquet,
        &["exec", &super::node_executable().to_string_lossy(), "-e", &detached_script],
        "process.exit(code)",
    )
    .spawn()
    .expect("spawn node launching pacquet exec");

    let connection = accept_detached_connection(&listener, &mut node_process);
    node_process.kill().expect("kill node");
    let _ = node_process.wait();
    assert_connection_closes(connection);

    drop(root);
}
