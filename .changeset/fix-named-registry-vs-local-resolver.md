---
"@pnpm/resolving.default-resolver": patch
"pnpm": patch
---

Fixed `pnpm add <alias>:@scope/pkg` for [named registries](https://github.com/pnpm/pnpm/pull/11324). The local resolver was claiming any specifier containing `/` as a local directory, so `pnpm add bit:@teambit/bit` (with `bit` configured under `namedRegistries`) installed a bogus link to `bit:@teambit/bit/` instead of resolving from the configured registry. The local resolver now runs after the named-registry resolver in the resolution chain.
