---
"@pnpm/bins.remover": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

pnpm no longer fails to remove a dependency whose `package.json` is malformed. It removes the dependency and keeps its bins [pnpm/tasks#295](https://github.com/pnpm/tasks/issues/295).
