---
"pacquet": minor
---

With `cargo.enabled`, `pnpm install` now installs the Rust toolchain named in `rust-toolchain.toml`. pnpm verifies the release signature, stores the toolchain once per machine, and links it into `.pnpm/rust`. `pnpm run` and `pnpm exec` put its `cargo` and `rustc` on the `PATH`.
