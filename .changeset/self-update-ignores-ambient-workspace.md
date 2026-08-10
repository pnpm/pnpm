---
"pacquet": patch
---

`pnpm self-update` no longer fails with `the installed pnpm wrapper is missing` when the global packages directory carries a `pnpm-workspace.yaml` of global settings (written there when a global install persists an `allowBuilds` decision). The engine install stays anchored to its own install directory instead of walking up and adopting that file as its workspace root. The `pnpm dlx` cache install gets the same anchoring, so a stray `pnpm-workspace.yaml` above the cache directory can no longer break it [#13697](https://github.com/pnpm/pnpm/issues/13697).
