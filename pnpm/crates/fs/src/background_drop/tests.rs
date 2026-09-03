use super::background_drop;
use std::{
    sync::mpsc::{Receiver, RecvTimeoutError, Sender},
    thread::{self, ThreadId},
    time::Duration,
};

struct DropProbe {
    dropped_on: Sender<ThreadId>,
}

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.dropped_on.send(thread::current().id()).expect("report the dropping thread");
    }
}

const REPORT_TIMEOUT: Duration = Duration::from_secs(30);

#[test]
fn drops_the_value_once_off_the_calling_thread() {
    let (dropped_on, reports): (Sender<ThreadId>, Receiver<ThreadId>) = std::sync::mpsc::channel();
    background_drop(DropProbe { dropped_on });
    let dropping_thread = reports.recv_timeout(REPORT_TIMEOUT).expect("the value is dropped");
    assert_ne!(dropping_thread, thread::current().id());
    // The probe owned the only sender, so its drop disconnects the
    // channel: one report, then no more.
    assert_eq!(reports.recv_timeout(REPORT_TIMEOUT).unwrap_err(), RecvTimeoutError::Disconnected);
}
