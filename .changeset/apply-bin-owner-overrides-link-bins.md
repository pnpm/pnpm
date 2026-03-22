---
"@pnpm/bins.resolver": patch
"@pnpm/bins.linker": patch
"pnpm": patch
---

Added `BIN_OWNER_OVERRIDES` and `pkgOwnsBin` to `@pnpm/bins.resolver`. Applied in bins.linker conflict resolution for consistent behavior between global conflict checking and actual bin linking, so packages like `npm` get priority for bins like `npx` even in non-global installs [#10850](https://github.com/pnpm/pnpm/issues/10850).

Removed the redundant `ownName` field from `CommandInfo` since `pkgOwnsBin` already handles the `binName === pkgName` case.
