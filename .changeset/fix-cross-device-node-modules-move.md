---
"pacquet": patch
---

`pnpm install` no longer fails with "Invalid cross-device link" while preserving a package's nested `node_modules` directory. This happened in Docker builds, where the directory comes from an earlier layer and OverlayFS refuses to rename it [#14758](https://github.com/pnpm/pnpm/issues/14758).
