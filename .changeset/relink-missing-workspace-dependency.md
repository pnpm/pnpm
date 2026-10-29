---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now relinks a `workspace:` or `link:` dependency whose link was deleted from `node_modules`. It used to report "Already up to date" and leave the link missing [#17485](https://github.com/pnpm/pnpm/issues/17485).
