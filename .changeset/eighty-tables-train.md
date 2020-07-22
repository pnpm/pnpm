---
"pnpm": patch
---

Should not print colored output when `color` is set to `never`. This was an issue in commands that don't use `@pnpm/default-reporter`. Commands like `pnpm list`, `pnpm outdated`.
