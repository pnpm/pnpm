use std::ffi::c_long;

// `pnpm-ffi-mimalloc`'s `purge-freed-memory-immediately` feature sets the
// default purge delay before `main`.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub(super) fn purge_delay() -> c_long {
    pnpm_ffi_mimalloc::purge_delay()
}
