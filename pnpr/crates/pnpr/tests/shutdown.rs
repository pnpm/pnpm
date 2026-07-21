#![cfg(unix)]

use std::{
    io::{Read as _, Write as _},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const PROCESS_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const IO_SLICE: Duration = Duration::from_millis(250);
const SPAWN_ATTEMPTS: usize = 5;

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn child_mut(&mut self) -> &mut Child {
        self.0.as_mut().expect("child guard is armed")
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn remaining(deadline: Instant) -> Option<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    (!remaining.is_zero()).then_some(remaining)
}

fn responds_to_ping(addr: SocketAddr, deadline: Instant) -> bool {
    let Some(timeout) = remaining(deadline) else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout.min(IO_SLICE)) else {
        return false;
    };

    let Some(timeout) = remaining(deadline) else {
        return false;
    };
    if stream.set_write_timeout(Some(timeout.min(IO_SLICE))).is_err()
        || stream
            .write_all(b"GET /-/ping HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .is_err()
    {
        return false;
    }

    let mut response = Vec::new();
    loop {
        let Some(timeout) = remaining(deadline) else {
            return false;
        };
        if stream.set_read_timeout(Some(timeout.min(IO_SLICE))).is_err() {
            return false;
        }
        let mut chunk = [0_u8; 512];
        let read = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return false,
            Ok(read) => read,
        };
        response.extend_from_slice(&chunk[..read]);
        if response.windows(2).any(|window| window == b"\r\n") {
            return response.starts_with(b"HTTP/1.1 200 ");
        }
        if response.len() > 4096 {
            return false;
        }
    }
}

enum Readiness {
    Ready,
    Exited(ExitStatus),
}

fn wait_until_ready(child: &mut ChildGuard, addr: SocketAddr) -> Readiness {
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    loop {
        if let Some(status) = child.child_mut().try_wait().expect("poll pnpr readiness") {
            child.disarm();
            return Readiness::Exited(status);
        }
        if responds_to_ping(addr, deadline) {
            return Readiness::Ready;
        }
        let Some(remaining) = remaining(deadline) else {
            panic!("pnpr did not serve /-/ping within {PROCESS_TIMEOUT:?}");
        };
        thread::sleep(remaining.min(POLL_INTERVAL));
    }
}

/// Reserving an ephemeral port and re-binding it from pnpr is inherently
/// racy: between `drop(listener)` and pnpr's own bind, another process can
/// claim the port. pnpr cannot advertise an OS-assigned address (`--listen
/// 127.0.0.1:0` would also bake port 0 into `public_url`), so instead the
/// reserve/spawn cycle retries on a fresh port whenever pnpr exits before
/// becoming ready.
fn spawn_ready_pnpr(root: &Path) -> ChildGuard {
    let storage = root.join("storage");
    let home = root.join("home");
    let xdg_config_home = root.join("xdg-config");
    std::fs::create_dir_all(&storage).expect("create isolated storage");
    std::fs::create_dir_all(&home).expect("create isolated home");
    std::fs::create_dir_all(&xdg_config_home).expect("create isolated config home");

    let mut last_exit = None;
    for _ in 0..SPAWN_ATTEMPTS {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve loopback port");
        let addr = listener.local_addr().expect("read reserved address");
        drop(listener);

        let child = Command::new(env!("CARGO_BIN_EXE_pnpr"))
            .arg("--listen")
            .arg(addr.to_string())
            .arg("--storage")
            .arg(&storage)
            .current_dir(root)
            .env("XDG_CONFIG_HOME", &xdg_config_home)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn pnpr");
        let mut child = ChildGuard(Some(child));

        match wait_until_ready(&mut child, addr) {
            Readiness::Ready => return child,
            Readiness::Exited(status) => last_exit = Some(status),
        }
    }
    panic!(
        "pnpr exited before serving /-/ping on {SPAWN_ATTEMPTS} fresh ports; last exit: {last_exit:?}",
    );
}

fn wait_for_exit(child: &mut ChildGuard) -> ExitStatus {
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    loop {
        if let Some(status) = child.child_mut().try_wait().expect("poll pnpr shutdown") {
            return status;
        }
        let Some(remaining) = remaining(deadline) else {
            panic!("pnpr did not stop within {PROCESS_TIMEOUT:?}");
        };
        thread::sleep(remaining.min(POLL_INTERVAL));
    }
}

#[test]
fn sigterm_stops_server_gracefully() {
    let root = TempDir::new().expect("create isolated pnpr root");
    let mut child = spawn_ready_pnpr(root.path());
    let pid = child.child_mut().id() as libc::pid_t;
    // SAFETY: The PID belongs to the guarded, unreaped child, so the OS cannot recycle it before this call.
    let result = unsafe { libc::kill(pid, libc::SIGTERM) };
    assert_eq!(result, 0, "send SIGTERM: {}", std::io::Error::last_os_error());

    let status = wait_for_exit(&mut child);
    child.disarm();
    assert!(status.success(), "pnpr exited unsuccessfully after SIGTERM: {status}");
}
