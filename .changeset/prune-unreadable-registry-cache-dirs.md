---
"@pnpm/cache.api": patch
"pnpm": patch
"pacquet": patch
---

`pnpm cache list-registries` now prints the registry URL instead of the cache directory name, matching `pnpm cache view`. It printed `https%3A+registry.npmjs.org` before and prints `https://registry.npmjs.org/` now [#15046](https://github.com/pnpm/pnpm/issues/15046).

Added `pnpm cache prune`, which deletes metadata cache directories this version of pnpm can no longer read. Upgrading to pnpm 12.4.0 changed the directory name for every registry, and the directory written under the old name was left behind with no way to remove it.
