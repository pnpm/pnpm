//! The architecture of the machine, as distinct from the architecture pnpm
//! itself was built for.

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
/// It differs from pnpm's own when pnpm runs emulated, such as an x64 pnpm
/// on ARM64 Windows. `None` when Windows cannot tell.
#[must_use]
pub fn native_architecture() -> Option<&'static str> {
    let is_wow64_process2 = load_is_wow64_process2()?;
    let mut process_machine = 0;
    let mut native_machine = 0;
    // SAFETY: the pointers are to stack locals of exactly the type the
    // function writes, and both outlive the call. `GetCurrentProcess`
    // returns the current-process pseudo handle, which is always valid and
    // must not be closed.
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

/// [`IsWow64Process2`][IsWow64Process2-ext] is missing before Windows 10
/// version 1511, so it is looked up rather than linked, which would stop pnpm
/// from starting there.
///
/// [IsWow64Process2-ext]: https://learn.microsoft.com/en-us/windows/win32/api/wow64apiset/nf-wow64apiset-iswow64process2
fn load_is_wow64_process2() -> Option<IsWow64Process2> {
    // SAFETY: both names are NUL-terminated literals that `s!` builds, and
    // `kernel32` is loaded into every process for its lifetime, so the
    // module handle stays valid and the address the lookup returns stays
    // mapped.
    let address = unsafe {
        let kernel32 = GetModuleHandleA(s!("kernel32.dll"));
        if kernel32.is_null() {
            return None;
        }
        GetProcAddress(kernel32, s!("IsWow64Process2"))?
    };
    // SAFETY: `kernel32!IsWow64Process2` has this signature in every
    // Windows that exports it, and `GetProcAddress` returned the address of
    // that export rather than of an unrelated symbol because the name it
    // was asked for is that export's.
    Some(unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, IsWow64Process2>(address)
    })
}
