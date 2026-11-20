#[link(wasm_import_module = "pnpm_host")]
unsafe extern "C" {
    pub fn operation_start(pointer: *const u8, length: usize) -> i32;
    pub fn response_len(identifier: u32) -> i32;
    pub fn response_read(identifier: u32, pointer: *mut u8, length: usize) -> i32;
    pub fn operation_cancel(identifier: u32);
    pub fn wait_completion() -> i32;
    pub fn resource_close(handle: u32) -> i32;
}
