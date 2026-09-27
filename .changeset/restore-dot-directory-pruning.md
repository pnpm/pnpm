---
"pacquet": patch
---

Workspace discovery prunes dot-prefixed directories, so a `packages` pattern such as `**` no longer matches projects inside `.cache` and other hidden directories [#16250](https://github.com/pnpm/pnpm/issues/16250).
