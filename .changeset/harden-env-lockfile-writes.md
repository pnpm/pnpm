---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

`pnpm install` no longer lets a symlink swapped into the path of `pnpm-lock.yaml` during a config dependency update redirect the lockfile write to the symlink target [pnpm/pnpm#14322](https://github.com/pnpm/pnpm/issues/14322).
