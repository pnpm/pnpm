---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed workspace packages with SemVer build metadata being skipped when they match the requested range and have the same version precedence as the registry package [#2812](https://github.com/pnpm/pnpm/issues/2812).
