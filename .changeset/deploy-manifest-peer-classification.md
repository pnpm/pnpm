---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm deploy` now binds optional peers declared only in `peerDependenciesMeta` for linked workspace packages. The deployed manifest and lockfile also retain these peers when filtering dependency groups.
