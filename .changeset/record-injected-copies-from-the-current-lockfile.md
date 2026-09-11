---
"pacquet": patch
---

A script listed in `syncInjectedDepsAfterScripts` no longer fails with `ERR_PNPM_INJECTED_DEPS_SYNC_READ_DIR` in a workspace whose lockfile carries an injected copy of a package that no project depends on. `pnpm install` used to record that copy in `node_modules/.modules.yaml` even though it did not keep it.
