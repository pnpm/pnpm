---
"@pnpm/deps.graph-builder": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

Skip local `file:` protocol dependencies during `pnpm fetch`. This fixes an issue where `pnpm fetch` would fail in Docker builds when local directory dependencies were not available [#10460](https://github.com/pnpm/pnpm/issues/10460).
