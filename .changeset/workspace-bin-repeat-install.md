---
"@pnpm/deps.status": patch
"@pnpm/workspace.state": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now relinks the bins of a workspace package when its `bin` field changes, including with `--frozen-lockfile` and an unchanged lockfile. A filtered install relinks them in the selected projects, and the next install that covers the remaining projects relinks them there [pnpm/pnpm#17324](https://github.com/pnpm/pnpm/issues/17324).
