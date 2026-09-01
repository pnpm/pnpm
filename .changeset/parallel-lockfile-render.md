---
"pacquet": patch
---

Speed up writing `pnpm-lock.yaml` in large workspaces: the entries of the big lockfile sections (`importers`, `packages`, `snapshots`) are now key-sorted and rendered to YAML in parallel [#14352](https://github.com/pnpm/pnpm/issues/14352).
