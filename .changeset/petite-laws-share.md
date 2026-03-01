---
"@pnpm/config": patch
"pnpm": patch
---

Fixed `--frozen-lockfile` failing in projects with a `pnpm-workspace.yaml` that has no `packages` field [#10571](https://github.com/pnpm/pnpm/issues/10571).
