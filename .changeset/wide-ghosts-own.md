---
"@pnpm/core": minor
"@pnpm/config": minor
"@pnpm/plugin-commands-installation": patch
"@pnpm/plugin-commands-patching": patch
---

Add new setting `useCatalogs` with options `always`, `prefer`, and `manual` for controlling if and how dependencies are added to the default catalog.

- `always`: Only allows dependency versions from the catalog. Adding a dependency outside the catalog's version range will cause an error.
- `prefer`: Prefers catalog versions, but will fall back to direct dependencies if no compatible version is found.
- `manual` (default): Does not automatically add dependencies to the catalog.
