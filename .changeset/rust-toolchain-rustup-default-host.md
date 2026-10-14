---
"pacquet": patch
---

On Windows, `pnpm install` now installs the Rust toolchain for rustup's default host, so a machine set up with `rustup set default-host x86_64-pc-windows-gnu` gets the MinGW toolchain. Without a default host, pnpm installs the MSVC toolchain for the machine's architecture, as rustup does. An x64 pnpm on ARM64 Windows now installs the ARM64 toolchain [#16854](https://github.com/pnpm/pnpm/issues/16854).
