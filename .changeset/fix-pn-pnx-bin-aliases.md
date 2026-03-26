---
"@pnpm/bins.resolver": patch
"@pnpm/engine.pm-commands": patch
"@pnpm/exe": patch
"pnpm": patch
---

Fixed `pn` and `pnx` short aliases not working when pnpm is installed via `@pnpm/exe`. The `BIN_OWNER_OVERRIDES` map was missing entries for the new aliases, `linkExePlatformBinary` in self-update was only creating the `pnpm` binary link, and the published `@pnpm/exe` tarball contained dead placeholder files for `pn`/`pnpx`/`pnx` that didn't work when preinstall scripts were skipped.
