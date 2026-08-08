use std::{fs, io, path::Path};

/// Publish `src` at `dest` without pulling an executable out from under a
/// concurrent process. A hard link avoids copying on the common same-filesystem
/// path; an atomic rename publishes either form only after it is complete.
pub(crate) fn replace_executable(src: &Path, dest: &Path) -> std::io::Result<()> {
    if same_file::is_same_file(src, dest).unwrap_or(false) {
        return Ok(());
    }
    let file_name = dest.file_name().unwrap_or(dest.as_os_str()).to_string_lossy().into_owned();
    let staged = dest.with_file_name(format!(".{file_name}.{}.pacquet-tmp", std::process::id()));
    match fs::remove_file(&staged) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let publish = || {
        if fs::hard_link(src, &staged).is_err() {
            let mut source = fs::File::open(src)?;
            let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&staged)?;
            io::copy(&mut source, &mut output)?;
            output.sync_all()?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))?;
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
