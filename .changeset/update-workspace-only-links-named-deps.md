---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm update --workspace` no longer links dependencies the user never named:

- Running it with `updateConfig.ignoreDependencies` configured no longer fails with `ERR_PNPM_WORKSPACE_PACKAGE_NOT_FOUND` for a dependency that is only published to the registry. Such dependencies keep their specifiers, as they already did when no dependencies were ignored.
- Passing package selectors that match no direct dependency no longer falls back to linking every workspace dependency.
