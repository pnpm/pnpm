---
"pacquet": minor
---

`pnpm add -g rust@<channel>` installs a Rust toolchain globally. The `cargo` and `rustc` commands run it outside projects that pin their own. `pnpm update -g`, `pnpm ls -g`, and `pnpm remove -g` manage it like any global package. In a project, `pnpm add rust@<channel>` pins the toolchain in `rust-toolchain.toml`.
