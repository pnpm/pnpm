---
"@pnpm/package-bins": minor
"@pnpm/link-bins": minor
"pnpm": minor
---

Added `BIN_OWNER_OVERRIDES` and `pkgOwnsBin` to `@pnpm/package-bins`. Applied in link-bins conflict resolution for consistent behavior between global conflict checking and actual bin linking, so packages like `npm` get priority for bins like `npx` even in non-global installs [#10850](https://github.com/pnpm/pnpm/issues/10850).
