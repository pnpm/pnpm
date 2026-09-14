---
"pacquet": patch
---

`pnpm install` in a single-project directory now detects a `package.json` edit that landed while the previous install was still finishing. Such an edit was reported as "Already up to date" while `pnpm install --frozen-lockfile` rejected the same working tree [#14890](https://github.com/pnpm/pnpm/issues/14890).
