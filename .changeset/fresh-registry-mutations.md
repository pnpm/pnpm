---
"pacquet": patch
"pnpm": patch
"@pnpm/registry-access.commands": patch
---

Fixed `pnpm deprecate`, `pnpm undeprecate`, and `pnpm unpublish` failing with revision conflicts when a registry cache serves stale package metadata.
