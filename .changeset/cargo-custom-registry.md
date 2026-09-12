---
"pacquet": minor
"@pnpm/pnpr": patch
---

pnpm can install Cargo dependencies from the sparse registry configured by `cargo.indexUrl`. The generated `Cargo.lock` records that registry as the source of every crate.
