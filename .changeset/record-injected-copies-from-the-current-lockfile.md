---
"pacquet": patch
---

`pnpm install` no longer records, in `node_modules/.modules.yaml`, an injected copy of a workspace package that no project depends on. The install created that copy and the virtual-store cleanup removed it again, so a script listed in `syncInjectedDepsAfterScripts` failed with `ERR_PNPM_INJECTED_DEPS_SYNC_READ_DIR` when it ran in that package.
