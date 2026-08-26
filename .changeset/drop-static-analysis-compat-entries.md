---
"pacquet": patch
---

The built-in compatibility database no longer adds dependencies that were detected by static analysis of published packages. Those entries named packages that are only imported for their types, so installing them was at best unnecessary and at worst broke the dependent: `@typescript-eslint/types` gained a `typescript` dependency resolved to the newest release, which put TypeScript 7 under older `@typescript-eslint` versions and made ESLint fail with "Cannot read properties of undefined (reading 'Intrinsic')". The database keeps its `@yarnpkg/extensions` entries and pnpm's own curated ones.
