---
"pnpm": patch
---

`pnpm install` now reads a `package.yaml` or `package.json5` manifest for a `file:` directory dependency. It does the same for an injected workspace project declared by path, such as `workspace:../lib`. The dependencies of such a package were not installed. `pnpm install --frozen-lockfile` then rejected the lockfile that `pnpm install` had written.
