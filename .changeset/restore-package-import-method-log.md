---
"@pnpm/core-loggers": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

Restored the store block a first install prints, naming how packages were materialized and where the stores live [#13315](https://github.com/pnpm/pnpm/issues/13315):

```text
Packages are hard linked from the content-addressable store to the virtual store.
  Content-addressable store is at: ~/.local/share/pnpm/store/v11
  Virtual store is at:             node_modules/.pnpm
```
