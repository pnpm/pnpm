---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fix peer dependency resolution dead lock [#8570](https://github.com/pnpm/pnpm/issues/8570). This change might change some of the keys in the `snapshots` field inside `pnpm-lock.yaml` but it should happen very rarely.
