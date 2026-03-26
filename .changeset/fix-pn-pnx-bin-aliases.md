---
"@pnpm/bins.resolver": patch
"@pnpm/exe": patch
"pnpm": patch
---

Fixed `pn` and `pnx` short aliases not working when pnpm is installed via `@pnpm/exe`. The `BIN_OWNER_OVERRIDES` map was missing entries for the new aliases, and the `@pnpm/exe` package was not including the bin placeholder files in its published tarball.
