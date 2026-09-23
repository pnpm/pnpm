---
"@pnpm/installing.deps-resolver": patch
"@pnpm/resolving.npm-resolver": patch
"pacquet": patch
"pnpm": patch
---

A dependency on an exact version now links the workspace package whose version adds build metadata to it, such as `1.0.0+abc` [#6483](https://github.com/pnpm/pnpm/issues/6483).
