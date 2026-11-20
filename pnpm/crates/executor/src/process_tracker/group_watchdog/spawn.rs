use super::{
    Command,
    GroupWatchdog,
};
use std::{
    io,
    os::fd::AsFd as _,
    process::Child,
};

pub(super) fn registered(command: &mut Command, watchdog: &GroupWatchdog) -> io::Result<Child> {
    // Platforms without `pipe2` mark pipes `FD_CLOEXEC` after creation, and
    // the announcement's hook forces `Command::spawn` onto `fork` rather than
    // `posix_spawn`, so hold the exclusive spawn guard across both the pipe
    // the announcement opens and the `fork`, to keep concurrent spawns from
    // inheriting an unclosed exec-error pipe.
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let spawning = pnpm_process::watchdog_spawning();
    let (spawned, announced) = command.spawn_announcing_group(watchdog.lifeline.as_fd());
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    drop(spawning);
    // A spawn that failed after the child announced its group leaves the
    // watchdog holding a group that will never exist.
    if let Some(group) = announced {
        watchdog.release(group)?;
    }
    spawned
}

#[cfg(test)]
mod tests;
