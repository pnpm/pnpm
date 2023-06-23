---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

In cases where both aliased and non-aliased dependencies exist to the same package, non-aliased dependencies will be used for resolving peer dependencies, addressing issue [#6588](https://github.com/pnpm/pnpm/issues/6588).
