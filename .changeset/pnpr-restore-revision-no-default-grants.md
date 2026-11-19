---
"@pnpm/pnpr": patch
---

Restoring an earlier registry revision with `POST /-/pnpr/v0/admin/registry-revisions/{version}/restore` no longer creates default grants for registries that a later revision removed. A restored registry with no grant now serves nobody until an admin adds a grant for it.
