---
"pacquet": patch
---

On Windows, `pnpm install` now installs the Rust toolchain for rustup's default host, so a machine set up with `rustup set default-host x86_64-pc-windows-gnu` gets the MinGW toolchain [#16854](https://github.com/pnpm/pnpm/issues/16854).
