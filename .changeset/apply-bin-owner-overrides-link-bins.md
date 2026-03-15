---
"@pnpm/link-bins": minor
"pnpm": minor
---

Applied `BIN_OWNER_OVERRIDES` in link-bins conflict resolution. This ensures consistent behavior between global conflict checking and actual bin linking, so packages like `npm` get priority for bins like `npx` even in non-global installs [#10850](https://github.com/pnpm/pnpm/issues/10850).
