---
"pacquet": patch
---

`pnpm install` now writes the source replacement for vendored crates into `.pnpm/crates/config.toml` and includes it as optional from `.cargo/config.toml`. A checkout without `.pnpm` builds with plain Cargo [#16659](https://github.com/pnpm/pnpm/issues/16659).
