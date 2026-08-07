---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Removing a dependency from `package.json` and reinstalling no longer re-resolves the dependency graph. The importer's entry is dropped from `pnpm-lock.yaml` and anything it made unreachable is pruned, which needs no registry access. Installs still fall back to a full resolution when another package resolves a peer dependency through the removed one, since that would change the dependent's entry rather than only prune.
