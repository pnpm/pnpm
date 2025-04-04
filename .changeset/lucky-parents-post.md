---
"@pnpm/fs.find-packages": patch
"pnpm": patch
---

Do not hang indefinitely, when there is a glob that starts with `!/` in `pnpm-workspace.yaml`. This fixes a regression introduced by [#9169](https://github.com/pnpm/pnpm/pull/9169).
