use crate::host;
use serde::Deserialize;
use serde_json::json;
use std::io;

#[derive(Deserialize)]
pub struct HostSignal {
    pub signal: String,
    pub number: i32,
    #[serde(default, rename = "sharedWithChildren")]
    pub shared_with_children: bool,
}

pub fn next_signal() -> io::Result<HostSignal> {
    let response = host::request(&json!({"operation": "signal.next"}))?;
    serde_json::from_value(response).map_err(io::Error::other)
}

pub fn kill(pid: u32) -> io::Result<()> {
    signal(pid, "SIGKILL")
}

pub fn signal(pid: u32, signal: &str) -> io::Result<()> {
    host::request(&json!({"operation": "process.killPid", "pid": pid, "signal": signal}))?;
    Ok(())
}
