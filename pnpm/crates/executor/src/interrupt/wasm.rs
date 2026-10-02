use super::{RELAYED_INTERRUPTS, RelayEntry, visit_targets};
use pnpm_process::HostSignal;
use std::{
    io,
    sync::{Once, atomic::Ordering},
    thread,
};

pub(super) fn install_handler() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        thread::spawn(listen);
    });
}

fn listen() {
    loop {
        match pnpm_process::next_signal() {
            Ok(signal) => relay_signal(&signal),
            Err(error) => {
                if error.kind() != io::ErrorKind::Interrupted {
                    eprintln!("Failed to listen for terminal signals: {error}");
                }
                return;
            }
        }
    }
}

#[expect(clippy::exit, reason = "an unhandled host signal ends the command")]
fn relay_signal(signal: &HostSignal) {
    let mut still_listening = false;
    let reached = visit_targets(|entry, target| {
        still_listening |= relay_to(entry, target, signal);
    });
    if !reached || !still_listening {
        std::process::exit(128 + signal.number);
    }
}

fn relay_to(entry: &RelayEntry, target: i32, signal: &HostSignal) -> bool {
    if entry.target.load(Ordering::Acquire) != target {
        return false;
    }
    let step = entry.relays.fetch_add(1, Ordering::Relaxed);
    if step >= RELAYED_INTERRUPTS {
        return false;
    }
    if step == 0 && signal.shared_with_children {
        return true;
    }
    let relayed = if step == 0 { signal.signal.as_str() } else { "SIGTERM" };
    if let Err(error) = pnpm_process::signal(target.unsigned_abs(), relayed)
        && error.kind() != io::ErrorKind::NotFound
    {
        eprintln!("Failed to relay {relayed} to process {target}: {error}");
    }
    true
}
