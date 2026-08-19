//! A hardened whole-file read for cache artifacts: refuses symlinks
//! and non-regular files on the opened descriptor itself, and bounds
//! how many bytes it will hold.
//!
//! A plain stat-then-read pair is a race: the path can be swapped
//! between the check and the open, redirecting the read through a
//! symlink or blocking it on a FIFO. Here the open itself refuses to
//! follow symlinks, refuses to block (a read-only FIFO open blocks
//! until a writer appears — `O_NONBLOCK` turns that into an immediate
//! open whose fstat then fails the regular-file check), and every
//! verdict afterwards is taken on the descriptor, which nothing can
//! swap.

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

/// Read `path` entirely, when it is a regular file of at most `cap`
/// bytes. `Ok(None)` when the file is absent; every refusal — symlink,
/// non-regular file, oversized — is an `Err` the caller may treat like
/// any other unreadable file.
pub fn read_regular_file_capped(path: &Path, cap: u64) -> io::Result<Option<Vec<u8>>> {
    let file = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut options = std::fs::OpenOptions::new();
            options.read(true).custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
            #[cfg(target_os = "linux")]
            options.custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            );
            match options.open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        #[cfg(not(unix))]
        {
            match File::open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            }
        }
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a regular file"));
    }
    if metadata.len() > cap {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} bytes exceeds the {cap}-byte cap", metadata.len()),
        ));
    }
    // `take(cap + 1)`: the size was read from the descriptor, but the
    // file can still grow under a concurrent writer — the bound holds
    // on the bytes actually read, not on a snapshot.
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let read = (&file).take(cap.saturating_add(1)).read_to_end(&mut bytes)?;
    if read as u64 > cap {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "grew past the cap mid-read"));
    }
    let _ = File::metadata; // keep the import shape uniform across cfgs
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests;
