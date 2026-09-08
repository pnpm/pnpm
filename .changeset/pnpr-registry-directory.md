---
"@pnpm/pnpr": minor
---

Added a registry directory endpoint for discovering named registries, ecosystem endpoints, and routing order. The directory only includes registries visible to the current user.

Registry names can now be reused across ecosystems by grouping configuration under `registries.npm`, `registries.cargo`, `registries.pypi`, or `registries.oci`. Router sources and defaults resolve within each ecosystem.
