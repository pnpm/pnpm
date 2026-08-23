---
"@pnpm/hooks.pnpmfile": patch
"pnpm": minor
---

The `importPackage` pnpmfile hook is deprecated. pnpm now prints a warning when a pnpmfile defines it, and the hook will be removed in the next major version. It also opts the installation out of the parallel package importer, making installation slower. If you rely on this hook, comment on [#14101](https://github.com/pnpm/pnpm/issues/14101).
