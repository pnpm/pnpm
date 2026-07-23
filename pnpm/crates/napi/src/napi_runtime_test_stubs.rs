//! No-op definitions of the napi runtime symbols the `#[napi]` trampolines
//! reference (the threadsafe-function call plus the error-reference release
//! pair).
//!
//! The Node host resolves these at addon load time, but a `cargo test` binary
//! has no host: without a definition it fails to link (macOS / Windows) or
//! fails the symbol lookup at load (Linux). These stubs make the unit-test
//! binary self-contained. They are compiled only for the test build
//! (`#[cfg(test)]` on the module declaration); the real cdylib addon excludes
//! them and binds the host's implementations. The pure-logic tests never enter
//! the napi runtime, so the stubs are never called.

// `no_mangle` is sound as documented above: test-only (`#[cfg(test)]`) so
// nothing else defines these names, and never called. A `// SAFETY:` prefix
// can't be used — clippy's `unnecessary_safety_comment` rejects it on a fn with
// no unsafe body.
#[unsafe(no_mangle)]
extern "C" fn napi_call_threadsafe_function() {}

#[unsafe(no_mangle)]
extern "C" fn napi_delete_reference() {}

#[unsafe(no_mangle)]
extern "C" fn napi_reference_unref() {}
