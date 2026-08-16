---
"@pnpm/types": minor
"@pnpm/config.reader": minor
"pnpm": minor
"pacquet": minor
---

Added `virtualStoreType`, which names where the virtual store lives — one store per machine, or one per project:

```yaml
virtualStoreType: global   # or: project
```

It is the canonical spelling of `enableGlobalVirtualStore`, which keeps working. When a project sets both, `virtualStoreType` wins. The default is unchanged: pnpm 12 installs into the global store, pnpm 11 into a project-local one.

The setting is independent of `nodeLinker`. `isolated` and `pnp` both work with either store type, and `hoisted` writes no virtual store at all, so it is unaffected.
