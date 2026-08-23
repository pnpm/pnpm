---
"pacquet": patch
---

`pnpm dedupe` in the Rust engine now fails with `ERR_PNPM_PEER_DEP_ISSUES` when `strictPeerDependencies` is set and unresolved peer dependency issues remain after deduplication, matching the TypeScript CLI [#14099](https://github.com/pnpm/pnpm/issues/14099). Previously it only ever printed a warning, regardless of the setting.
