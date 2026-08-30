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

It is the canonical spelling of `enableGlobalVirtualStore`, which keeps working. When a project sets both, `virtualStoreType` wins. It can also be set through `PNPM_CONFIG_VIRTUAL_STORE_TYPE` and read back with `pnpm config get virtualStoreType`. The default is unchanged — `project`, so the shared store stays opt-in.

The setting is independent of `nodeLinker`. `isolated` and `pnp` both work with either store type, and `hoisted` writes no virtual store at all, so it is unaffected.
