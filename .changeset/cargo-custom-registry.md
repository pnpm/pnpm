---
"pacquet": minor
"@pnpm/pnpr": patch
---

pnpm can install Cargo dependencies from the sparse registry configured by `cargo.indexUrl`. Cargo resolution through `pnprServer` preserves the configured registry in `Cargo.lock`.
