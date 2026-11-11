//! The `pnpm_host` import module: asynchronous operations the supervisor
//! runs on the guest's behalf.
//!
//! An operation is started with a JSON payload and identified by the
//! positive number `operation_start` answers with; a value at or below zero
//! is a failure. The supervisor keeps a result until it is read or
//! cancelled.

#[link(wasm_import_module = "pnpm_host")]
unsafe extern "C" {
    pub fn operation_start(pointer: *const u8, length: usize) -> i32;
    pub fn response_len(identifier: u32) -> i32;
    pub fn response_read(identifier: u32, pointer: *mut u8, length: usize) -> i32;
    pub fn operation_cancel(identifier: u32);
    pub fn wait_completion() -> i32;
    pub fn resource_close(handle: u32) -> i32;
}
