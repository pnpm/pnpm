//! How many threads the process runs, as the kernel reports it.

use std::io;

/// The number of threads the process runs now, the calling one included.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn thread_count() -> io::Result<usize> {
    const STAT: &str = "/proc/self/stat";
    let stat = std::fs::read_to_string(STAT)?;
    parse_stat_thread_count(&stat)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{STAT} has no thread count: {stat}"),
            )
        })
}

/// The `num_threads` field of a `/proc/<pid>/stat` line. The command name
/// in field 2 is in parentheses and can hold spaces and parentheses
/// itself, so the fields are counted from the last `)`. `num_threads` is
/// field 20, and the first field past the name is field 3.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn parse_stat_thread_count(stat: &str) -> Option<usize> {
    let (_, fields) = stat.rsplit_once(')')?;
    fields
        .split_whitespace()
        .nth(20 - 3)?
        .parse()
        .ok()
}

/// The number of threads the process runs now, the calling one included.
#[cfg(target_vendor = "apple")]
pub fn thread_count() -> io::Result<usize> {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let size = size_of::<libc::proc_taskinfo>();
    // SAFETY: the pointer is to a stack local of exactly the type the
    // `PROC_PIDTASKINFO` flavor writes, and `size` is its size, so the call
    // writes inside it. `getpid` takes no arguments and cannot fail.
    let written = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast(),
            libc::c_int::try_from(size).expect("proc_taskinfo fits in a c_int"),
        )
    };
    if written <= 0 {
        return Err(io::Error::last_os_error());
    }
    if usize::try_from(written).ok() != Some(size) {
        return Err(short_write("proc_pidinfo", written, size));
    }
    // SAFETY: the call reported writing the whole structure.
    let info = unsafe { info.assume_init() };
    usize::try_from(info.pti_threadnum)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// The number of threads the process runs now, the calling one included.
#[cfg(target_os = "freebsd")]
pub fn thread_count() -> io::Result<usize> {
    // SAFETY: `getpid` takes no arguments and cannot fail.
    let pid = unsafe { libc::getpid() };
    let name = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
    // Without `KERN_PROC_INC_THREAD`, the kernel answers with one record for
    // the whole process, whose `ki_numthreads` counts every thread.
    let info: libc::kinfo_proc = read_sysctl(&name)?;
    usize::try_from(info.ki_numthreads)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// The number of threads the process runs now, the calling one included.
#[cfg(target_os = "netbsd")]
pub fn thread_count() -> io::Result<usize> {
    // SAFETY: `getpid` takes no arguments and cannot fail.
    let pid = unsafe { libc::getpid() };
    let record_size =
        libc::c_int::try_from(size_of::<libc::kinfo_proc2>()).expect("kinfo_proc2 fits in a c_int");
    // `KERN_PROC2` takes the record size and count in the name, so an
    // older kernel with a shorter record still answers.
    let name = [libc::CTL_KERN, libc::KERN_PROC2, libc::KERN_PROC_PID, pid, record_size, 1];
    let info: libc::kinfo_proc2 = read_sysctl(&name)?;
    usize::try_from(info.p_nlwps).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Read the `sysctl` node `name` into a fresh `Record`, which must be the
/// type the node answers with.
#[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
fn read_sysctl<Record: Copy>(name: &[libc::c_int]) -> io::Result<Record> {
    let mut record = std::mem::MaybeUninit::<Record>::uninit();
    let size = size_of::<Record>();
    let mut written = size;
    // SAFETY: `name` is a live slice of `name.len()` integers. The output
    // pointer is to a stack local of `size` bytes, which is the length the
    // call is told it has, so the kernel writes inside it and reports in
    // `written` how much it wrote. No new value is passed.
    let status = unsafe {
        libc::sysctl(
            name.as_ptr(),
            libc::c_uint::try_from(name.len()).expect("a sysctl name fits in a c_uint"),
            record.as_mut_ptr().cast(),
            &raw mut written,
            std::ptr::null(),
            0,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    if written != size {
        return Err(short_write("sysctl", written, size));
    }
    // SAFETY: the kernel wrote all `size` bytes of the record.
    Ok(unsafe { record.assume_init() })
}

#[cfg(any(target_vendor = "apple", target_os = "freebsd", target_os = "netbsd"))]
fn short_write(call: &str, written: impl std::fmt::Display, expected: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{call} wrote {written} bytes of process information, expected {expected}"),
    )
}

/// Platforms with no way of counting threads here refuse every write.
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "netbsd",
)))]
pub fn thread_count() -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "counting threads is not supported on this platform",
    ))
}

#[cfg(test)]
mod tests;
