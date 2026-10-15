use windows_sys::{
    Win32::{
        Foundation::HANDLE,
        System::{
            LibraryLoader::{GetModuleHandleA, GetProcAddress},
            SystemInformation::{
                IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64,
                IMAGE_FILE_MACHINE_I386,
            },
            Threading::GetCurrentProcess,
        },
    },
    core::{BOOL, s},
};

type IsWow64Process2 =
    unsafe extern "system" fn(HANDLE, *mut IMAGE_FILE_MACHINE, *mut IMAGE_FILE_MACHINE) -> BOOL;

/// The architecture of the machine, as `std::env::consts::ARCH` names it.
/// It differs from pnpm's own when pnpm runs emulated, such as an x64
/// pnpm on ARM64 Windows. `None` when Windows cannot tell.
pub(super) fn native_architecture() -> Option<&'static str> {
    // `IsWow64Process2` is missing before Windows 10 version 1511, so it is
    // looked up instead of linked, which would stop pnpm from starting there.
    // SAFETY: the module and procedure names are NUL-terminated literals,
    // and kernel32 stays loaded for the life of the process.
    let function = unsafe {
        let kernel32 = GetModuleHandleA(s!("kernel32.dll"));
        if kernel32.is_null() {
            return None;
        }
        GetProcAddress(kernel32, s!("IsWow64Process2"))?
    };
    // SAFETY: `IsWow64Process2` has this signature in every Windows that
    // exports it.
    let is_wow64_process2: IsWow64Process2 = unsafe { std::mem::transmute(function) };
    let mut process_machine = 0;
    let mut native_machine = 0;
    // SAFETY: the current-process pseudo handle needs no closing, and both
    // pointers are stack locals that outlive the call.
    let succeeded = unsafe {
        is_wow64_process2(GetCurrentProcess(), &raw mut process_machine, &raw mut native_machine)
    };
    if succeeded == 0 {
        return None;
    }
    match native_machine {
        IMAGE_FILE_MACHINE_AMD64 => Some("x86_64"),
        IMAGE_FILE_MACHINE_ARM64 => Some("aarch64"),
        IMAGE_FILE_MACHINE_I386 => Some("x86"),
        _ => None,
    }
}
