---
"pacquet": patch
---

`pnpm install` no longer aborts on a failed allocation of many gigabytes when peer dependency ranges combine many overlapping `||` alternatives [pnpm/pnpm#15867](https://github.com/pnpm/pnpm/issues/15867).
