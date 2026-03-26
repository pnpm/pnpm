---
"@pnpm/bins.resolver": patch
"@pnpm/engine.pm-commands": patch
"pnpm": patch
---

Fixed `pn` and `pnx` short aliases not working when pnpm is installed via `@pnpm/exe`. The `BIN_OWNER_OVERRIDES` map was missing entries for the new aliases, and `linkExePlatformBinary` in self-update was only creating the `pnpm` binary link without also creating `pn`, `pnpx`, and `pnx`.
