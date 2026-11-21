//! Keep pnpm's own standard handles out of the processes it spawns.
//!
//! A process inherits its standard handles marked inheritable, and
//! `CreateProcess` with `bInheritHandles` passes every inheritable handle to
//! the child. Rust's `Stdio::inherit` gives the child an inheritable duplicate
//! as its standard handle, so the originals reach it as stray handles that
//! nothing in the child closes. A detached grandchild, such as the Nx daemon,
//! keeps them open, and a shell reading pnpm's redirected output waits until
//! that grandchild exits. [`disable`] does what Node.js does at startup and
//! marks the originals non-inheritable. On Unix the child's standard streams
//! are the same descriptors, so no stray copy exists and [`disable`] is a
//! no-op.

#[cfg(windows)]
pub fn disable() {
    pnpm_ffi_terminal::stop_inheriting_standard_handles();
}

#[cfg(not(windows))]
pub fn disable() {}
