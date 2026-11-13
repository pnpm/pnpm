---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm import` now preserves locked versions for `npm:` aliased dependencies when importing from `package-lock.json`, `npm-shrinkwrap.json`, or `yarn.lock`. `pnpm import` also parses Yarn Classic `yarn.lock` files that reference `__metadata` package names without treating them as Yarn Berry lockfiles.
