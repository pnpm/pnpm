// The resolver makes millions of small, short-lived allocations, which
// mimalloc serves much faster than the system allocators. The other release
// targets keep the system allocator until mimalloc's C build is verified there.
#[cfg(all(
    any(target_os = "linux", target_os = "macos", target_os = "windows"),
    any(target_arch = "x86_64", target_arch = "aarch64"),
))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> std::process::ExitCode {
    pnpm_cli::main()
}
