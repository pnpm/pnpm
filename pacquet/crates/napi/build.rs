fn main() {
    napi_build::setup();

    // The crate's unit-test binary links the whole library, which references
    // napi runtime symbols (`napi_call_threadsafe_function`,
    // `napi_delete_reference`, ...) that the Node host resolves at addon load
    // time. A standalone `cargo test` binary has no host, so on macOS and Linux
    // the link errors on those undefined symbols. Let them stay unresolved: the
    // addon itself already tolerates them (a `.dylib` / `.so` resolves them at
    // load), and the pure-logic tests never call into the napi runtime, so
    // nothing is actually missing when the tests run. Windows resolves the
    // symbols via `node.lib` and needs nothing here.
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos" | "ios") => println!("cargo:rustc-link-arg=-Wl,-undefined,dynamic_lookup"),
        Ok("windows") => {}
        _ => println!("cargo:rustc-link-arg=-Wl,--unresolved-symbols=ignore-all"),
    }
}
