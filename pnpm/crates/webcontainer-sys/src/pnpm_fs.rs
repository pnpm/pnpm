//! The `pnpm_fs` import module: filesystem operations the WASI preview-1
//! interface does not reach.
//!
//! Every function answers with an `errno`, or `0` for success, and writes
//! its result through the `output` pointer. A path is a pointer and a
//! length, in the host's encoding, and is borrowed only for the call.

#[link(wasm_import_module = "pnpm_fs")]
unsafe extern "C" {
    pub fn create_new(path: *const u8, length: usize, mode: u32, output: *mut u32) -> i32;
    pub fn open_nofollow(path: *const u8, length: usize, output: *mut u32) -> i32;
    pub fn open_directory_nofollow_beneath(
        path: *const u8,
        length: usize,
        template: *const u8,
        template_length: usize,
        output: *mut u32,
    ) -> i32;
    pub fn grant_directory_mode_beneath(
        path: *const u8,
        length: usize,
        template: *const u8,
        template_length: usize,
        extra: u32,
    ) -> i32;
    pub fn open_lock(path: *const u8, length: usize, output: *mut u32) -> i32;
    pub fn try_lock(descriptor: i32, exclusive: u32) -> i32;
    pub fn sqlite_register(path: *const u8, length: usize, registered: u32) -> i32;
    pub fn check_owner(descriptor: i32) -> i32;
    pub fn path_access_executable(path: *const u8, length: usize) -> i32;
    pub fn fchmod(descriptor: i32, mode: u32) -> i32;
    pub fn fmode(descriptor: i32, output: *mut u32) -> i32;
    pub fn lmode(path: *const u8, length: usize, output: *mut u32) -> i32;
    pub fn umask() -> u32;
    pub fn uid() -> u32;
    pub fn secure_directory(path: *const u8, length: usize) -> i32;
}
