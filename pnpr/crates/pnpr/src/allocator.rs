use std::ffi::{c_int, c_long};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// `mi_option_purge_delay` in `mimalloc.h`. `libmimalloc-sys` binds the
// options on either side of it but not this one.
const MI_OPTION_PURGE_DELAY: c_int = 15;

unsafe extern "C" {
    fn mi_option_set_default(option: c_int, value: c_long);
    fn mi_option_get(option: c_int) -> c_long;
}

// mimalloc purges freed memory only while a thread is allocating, and by
// default only after a delay. Memory freed by the last requests of a
// burst then stays resident for as long as the server is idle. Purging
// without a delay returns it at once. `MIMALLOC_PURGE_DELAY` overrides
// this.
//
// Call it before the process starts another thread.
pub(super) fn purge_freed_memory_immediately() {
    // SAFETY: `mimalloc` links the library that defines this function.
    // The index is a valid option in mimalloc v2 and v3. The function
    // writes the option without synchronization, and no other thread
    // exists yet to read it.
    unsafe { mi_option_set_default(MI_OPTION_PURGE_DELAY, 0) };
}

pub(super) fn purge_delay() -> c_long {
    // SAFETY: `mimalloc` links the library that defines this function, and
    // the index is a valid option in mimalloc v2 and v3.
    unsafe { mi_option_get(MI_OPTION_PURGE_DELAY) }
}
