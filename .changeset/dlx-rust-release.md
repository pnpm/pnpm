---
"pacquet": patch
---

`pnx --package=rust@<channel> <tool>` runs a tool of that Rust release, for example `pnx --package=rust@nightly-2026-01-01 cargo build`. pnpm installs the release with the components and targets from `rust-toolchain.toml` and the target of each `--target` argument.
