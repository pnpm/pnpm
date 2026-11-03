---
"pacquet": patch
---

`pnpm dedupe --check` no longer rewrites `pnpm-lock.yaml` while it checks, so the file keeps its inode and modification time and watchers are not triggered [pnpm/pnpm#17628](https://github.com/pnpm/pnpm/issues/17628).
