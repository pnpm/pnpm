---
"@pnpm/config.normalize-registries": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/cache.api": patch
"pnpm": patch
"pacquet": patch
---

Prevent registry metadata cache collisions between registries sharing a host but using different URL paths [pnpm/pnpm#13558](https://github.com/pnpm/pnpm/issues/13558).
