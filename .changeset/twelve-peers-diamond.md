---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed inconsistent resolution of a peer dependency that is shared through a diamond. When a package peer-depends both another package and one of that package's own peer dependencies (for example `@typescript-eslint/eslint-plugin` peer-depends both `@typescript-eslint/parser` and `typescript`, and `@typescript-eslint/parser` peer-depends `typescript`), pnpm no longer reuses a hoisted instance of the shared peer that was resolved against a different version [#12079](https://github.com/pnpm/pnpm/issues/12079).
