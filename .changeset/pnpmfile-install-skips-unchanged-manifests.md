---
"pacquet": patch
---

`pnpm install` in an up-to-date workspace with a pnpmfile or `configDependencies` no longer reads the `package.json` of the other workspace projects whose files did not change since the last install [pnpm/tasks#256](https://github.com/pnpm/tasks/issues/256).
