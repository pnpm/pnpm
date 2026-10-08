---
"pacquet": minor
---

`pnpm shim add rust` adds project-aware shims for `cargo`, `rustc`, and the other Rust tools. In a project with a `rust-toolchain.toml`, they run the toolchain the file names and install it on first use. Elsewhere, the next command of the same name on `PATH` runs, such as rustup's.
