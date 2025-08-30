---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

When resolving peer dependencies, pnpm looks whether the peer dependency is present in the root workspace project's dependencies. This change makes it so that the peer dependency is correctly resolved even from aliased npm-hosted dependencies or other types of dependencies [#9913](https://github.com/pnpm/pnpm/issues/9913).
