---
"@pnpm/core": minor
"@pnpm/config": minor
"@pnpm/plugin-commands-installation": minor
"@pnpm/plugin-commands-patching": minor
---

A new `catalogMode` setting is available for controlling if and how dependencies are added to the default catalog. It can be configured to several modes:

- `strict`: Only allows dependency versions from the catalog. Adding a dependency outside the catalog's version range will cause an error.
- `prefer`: Prefers catalog versions, but will fall back to direct dependencies if no compatible version is found.
- `manual` (default): Does not automatically add dependencies to the catalog.
