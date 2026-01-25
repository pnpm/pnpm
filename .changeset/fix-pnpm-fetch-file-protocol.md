---
"@pnpm/deps.graph-builder": patch
"@pnpm/headless": patch
"@pnpm/core": patch
"@pnpm/plugin-commands-installation": patch
"pnpm": patch
---

Skip local `file:` protocol dependencies during `pnpm fetch`. This fixes an issue where `pnpm fetch` would fail in Docker builds when local directory dependencies were not available [#10460](https://github.com/pnpm/pnpm/issues/10460).
