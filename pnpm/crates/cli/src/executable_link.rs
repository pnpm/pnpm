use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

/// Publish `src` at `dest` without pulling an executable out from under a
/// concurrent process. A hard link avoids copying on the common same-filesystem
/// path; an atomic rename publishes either form only after it is complete.
pub(crate) fn replace_executable(src: &Path, dest: &Path) -> std::io::Result<()> {
    if same_file::is_same_file(src, dest).unwrap_or(false) {
        return Ok(());
    }
    // Process id alone is not unique enough: bin linking runs on rayon,
    // so two in-process publishes of the same destination must not share
    // a staging path.
    static STAGED_SEQ: AtomicU64 = AtomicU64::new(0);
    let file_name = dest.file_name().unwrap_or(dest.as_os_str()).to_string_lossy().into_owned();
    let staged = dest.with_file_name(format!(
        ".{file_name}.{}.{}.pacquet-tmp",
        std::process::id(),
        STAGED_SEQ.fetch_add(1, Ordering::Relaxed),
    ));
    let publish = || {
        // A hard link shares the source's inode, so it is only usable
        // when the source already carries the executable bits — a chmod
        // through the link would mutate the source (the running
        // executable, or a store entry, possibly in a read-only store).
        // A non-executable source is copied instead, and only the fresh
        // copy gets its mode set.
        #[cfg(unix)]
        let src_is_executable = {
            use std::os::unix::fs::PermissionsExt as _;
            fs::metadata(src).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 == 0o111)
        };
        #[cfg(not(unix))]
        let src_is_executable = true;
        if !(src_is_executable && fs::hard_link(src, &staged).is_ok()) {
            let mut source = fs::File::open(src)?;
            let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&staged)?;
            io::copy(&mut source, &mut output)?;
            output.sync_all()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
            }
        }
        swap_into_place(&staged, dest)
    };
    publish().inspect_err(|_| {
        let _ = fs::remove_file(&staged);
    })
}

fn swap_into_place(staged: &Path, dest: &Path) -> std::io::Result<()> {
    const ATTEMPTS: usize = 10;
    const BACKOFF: std::time::Duration = std::time::Duration::from_millis(25);

    for attempt in 1..ATTEMPTS {
        match fs::rename(staged, dest) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(error),
            Err(_) => std::thread::sleep(BACKOFF * u32::try_from(attempt).unwrap_or(1)),
        }
    }
    fs::rename(staged, dest)
}
