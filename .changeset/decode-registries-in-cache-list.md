---
"@pnpm/cache.api": patch
"pnpm": patch
"pacquet": patch
---

`pnpm cache list-registries` now prints the registry URL, matching `pnpm cache view`. It printed `https%3A+registry.npmjs.org` before and prints `https://registry.npmjs.org/` now [#15046](https://github.com/pnpm/pnpm/issues/15046).
