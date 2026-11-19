---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed `pnpm install --lockfile-only` fetching unchanged Git dependencies when an unrelated dependency changes and `trustLockfile` is enabled [#18217](https://github.com/pnpm/pnpm/issues/18217).
