---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

When the same dependency with missing peers is used in multiple workspace projects, install the missing peers in each workspace project [#4820](https://github.com/pnpm/pnpm/issues/4820).
