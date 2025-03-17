---
"@pnpm/resolver-base": minor
"@pnpm/npm-resolver": major
---

The `@pnpm/npm-resolver` package can now return `workspace` in the `resolvedVia` field of its results. This will be the case if the resolved package was requested through the `workspace:` protocol or if the wanted dependency's name and specifier match a package in the workspace. Previously, the `resolvedVia` field was always set to `local-filesystem` for workspace packages.
