---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Removing a dependency from `package.json` and reinstalling no longer re-resolves the dependency graph. The importer's entry is dropped from `pnpm-lock.yaml`, anything it made unreachable is pruned, and a catalog entry that loses its last referent is removed — all without registry access. Installs still fall back to a full resolution when a package that stays resolves a peer dependency through the removed one, since that would change the surviving package's entry rather than only prune.
