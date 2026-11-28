---
"pnpm": minor
---

On a machine without rustup, the Rust shims that `pnpm shim add rust` and `pnpm add -g rust` create now run the release that a `+<toolchain>` argument or `RUSTUP_TOOLCHAIN` names, such as `cargo +nightly build`. pnpm installs the release into the store first. A `RUSTUP_TOOLCHAIN` that is the path of a toolchain directory runs the tool from that directory.
