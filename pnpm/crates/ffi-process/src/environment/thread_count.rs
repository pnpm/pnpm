//! How many threads the process runs, as the kernel reports it.
//!
//! Every function here answers `None` when the count cannot be read.

/// The number of threads the process runs now, the calling one included.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn thread_count() -> Option<usize> {
    parse_stat_thread_count(&std::fs::read("/proc/self/stat").ok()?)
}

/// The `num_threads` field of a `/proc/<pid>/stat` line.
///
/// - The command name in field 2 is in parentheses and can hold any byte,
///   so the fields are counted from the last `)`. `num_threads` is field
///   20, and the first field past the name is field 3.
/// - A count of 0 is no count: QEMU user-mode emulation before 9.1
///   reports 0 there.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn parse_stat_thread_count(stat: &[u8]) -> Option<usize> {
    let name_end = stat
        .iter()
        .rposition(|&byte| byte == b')')?;
    let fields = std::str::from_utf8(&stat[name_end + 1..]).ok()?;
    let count = fields
        .split_whitespace()
        .nth(20 - 3)?
        .parse()
        .ok()?;
    (count > 0).then_some(count)
}

/// The number of threads the process runs now, the calling one included.
#[cfg(target_vendor = "apple")]
pub fn thread_count() -> Option<usize> {
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
            libc::c_int::try_from(size).ok()?,
        )
    };
    if usize::try_from(written).ok() != Some(size) {
        return None;
    }
    // SAFETY: the call wrote the whole structure, and every field of
    // `proc_taskinfo` is an integer, valid for any bytes.
    let info = unsafe { info.assume_init() };
    usize::try_from(info.pti_threadnum).ok()
}

/// The number of threads the process runs now, the calling one included.
#[cfg(target_os = "freebsd")]
pub fn thread_count() -> Option<usize> {
    // SAFETY: `getpid` takes no arguments and cannot fail.
    let pid = unsafe { libc::getpid() };
    // Without `KERN_PROC_INC_THREAD`, the kernel answers with one record for
    // the whole process, whose `ki_numthreads` counts every thread.
    let name = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
    // SAFETY: the node answers with a `kinfo_proc`, whose fields are
    // integers, arrays of them, and raw pointers, all valid for any bytes.
    let info: libc::kinfo_proc = unsafe { read_sysctl(&name) }?;
    usize::try_from(info.ki_numthreads).ok()
}

/// The number of threads the process runs now, the calling one included.
#[cfg(target_os = "netbsd")]
pub fn thread_count() -> Option<usize> {
    // SAFETY: `getpid` takes no arguments and cannot fail.
    let pid = unsafe { libc::getpid() };
    let record_size = libc::c_int::try_from(size_of::<libc::kinfo_proc2>()).ok()?;
    // `KERN_PROC2` takes the record size and count in the name.
    let name = [libc::CTL_KERN, libc::KERN_PROC2, libc::KERN_PROC_PID, pid, record_size, 1];
    // SAFETY: the node answers with a `kinfo_proc2`, whose fields are
    // integers and arrays of them, all valid for any bytes.
    let info: libc::kinfo_proc2 = unsafe { read_sysctl(&name) }?;
    usize::try_from(info.p_nlwps).ok()
}

/// Read the `sysctl` node `name` into a fresh `Record`. `None` when the
/// call fails or writes less than a whole record.
///
/// # Safety
///
/// `Record` is the type the node answers with, and every bit pattern is a
/// valid `Record`.
#[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
unsafe fn read_sysctl<Record>(name: &[libc::c_int]) -> Option<Record> {
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
            libc::c_uint::try_from(name.len()).ok()?,
            record.as_mut_ptr().cast(),
            &raw mut written,
            std::ptr::null(),
            0,
        )
    };
    if status != 0 || written != size {
        return None;
    }
    // SAFETY: the kernel wrote all `size` bytes, and the caller guarantees
    // that any bytes are a valid `Record`.
    Some(unsafe { record.assume_init() })
}

/// No platform API to count threads with.
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    target_os = "freebsd",
    target_os = "netbsd",
)))]
pub fn thread_count() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests;
