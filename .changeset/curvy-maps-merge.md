---
"pacquet": patch
---

`pnpm install` now merges Git conflict markers in `pnpm-lock.yaml`. It parses both sides of the conflict and keeps the versions they locked. A conflict in the config dependencies recorded at the top of the lockfile is merged too [#14880](https://github.com/pnpm/pnpm/issues/14880).
