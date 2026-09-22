---
"pacquet": patch
---

Fixed `pnpm update --no-save` bypassing version-scoped overrides when a dependency selector specifies a version. The generated lockfile remains compatible with `pnpm install --frozen-lockfile` [pnpm/pnpm#14923](https://github.com/pnpm/pnpm/issues/14923).
