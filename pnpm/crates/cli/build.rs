fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Most commands never call into these system DLLs, so pnpm.exe loads each
    // one on first use instead of at every start. The list is what the release
    // binary imports, minus the DLLs and API sets every process loads anyway.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        for dll in
            ["combase", "crypt32", "mpr", "oleaut32", "shell32", "user32", "userenv", "ws2_32"]
        {
            println!("cargo:rustc-link-arg-bin=pnpm=/DELAYLOAD:{dll}.dll");
        }
        println!("cargo:rustc-link-arg-bin=pnpm=delayimp.lib");
    }
}
