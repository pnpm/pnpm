use std::thread;

/// Move `value`'s teardown off the calling thread.
///
/// For a value whose drop only returns memory — a workspace-scale map,
/// a serialized document tree — freeing it on the critical path buys
/// nothing; a detached thread takes the drop instead. A host that
/// cannot take another thread pays the drop inline: a failed
/// [`thread::Builder::spawn`] drops the closure, value and all, right
/// here.
///
/// Only for values whose teardown is pure deallocation. The thread is
/// detached, so process exit may cut the drop short — a value whose
/// drop flushes, unlocks, or signals must not go through here.
pub fn background_drop<Value: Send + 'static>(value: Value) {
    drop(thread::Builder::new().spawn(move || drop(value)));
}

#[cfg(test)]
mod tests;
