---
"pnpm": patch
---

Fixed catalog updates with explicit dist tags, so commands like `pnpm update package@beta` keep the dependency using `catalog:` in `package.json` and update the catalog entry instead of writing the resolved specifier directly to the project manifest.

This change is implemented for the TypeScript CLI path. Rust/pacquet parity is deferred because the triaged issue identifies this catalog update flow as TypeScript-only for now.
