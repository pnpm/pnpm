use std::ffi::c_long;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// mimalloc purges freed memory only while a thread is allocating, and by
// default only after a delay. Memory freed by the last requests of a
// burst then stays resident for as long as the server is idle. Purging
// without a delay returns it at once. `MIMALLOC_PURGE_DELAY` overrides
// this.
//
// Call it before the process starts another thread.
pub(super) fn purge_freed_memory_immediately() {
    pnpm_ffi_mimalloc::set_default_purge_delay(0);
}

pub(super) fn purge_delay() -> c_long {
    pnpm_ffi_mimalloc::purge_delay()
}
