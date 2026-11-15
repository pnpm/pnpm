use super::WatchdogAddress;
use crate::process::Command;
use std::{
    io::{BufRead, BufReader},
    process::Stdio,
};

/// A pid that names a process other than pnpm's child, as the watchdog's
/// pid can once the kernel reaped it, is never addressed.
#[test]
fn a_pid_that_is_not_a_child_is_not_addressed() {
    let mut parent = Command::new("sh")
        .args(["-c", "sleep 30 & echo $!; wait"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the parent of the process to address");
    let mut line = String::new();
    BufReader::new(parent.stdout.take().expect("piped stdout"))
        .read_line(&mut line)
        .expect("read the pid of the process to address");
    let grandchild: u32 = line.trim().parse().expect("a pid");

    let address = WatchdogAddress::of(grandchild);

    let _ = parent.kill();
    let _ = parent.wait();
    // SAFETY: `kill` only signals the grandchild started above.
    unsafe { libc::kill(i32::try_from(grandchild).expect("a pid fits i32"), libc::SIGKILL) };
    assert!(matches!(address, WatchdogAddress::Gone), "a grandchild should not be addressed");
}
