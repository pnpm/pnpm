---
"pacquet": minor
---

`pnpm add --config` now saves a version range in `pnpm-workspace.yaml` by the same rules as `pnpm add`, so `pnpm add --config my-config@latest` saves a range such as `^1.2.0`. It also resolves the package again when `pnpm-lock.yaml` already locks an older version [#16814](https://github.com/pnpm/pnpm/issues/16814).
