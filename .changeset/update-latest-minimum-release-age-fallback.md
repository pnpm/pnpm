---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm update --latest` now falls back to the newest mature version when an excluded dependency fails to install due to a maturity error [pnpm/pnpm#11068](https://github.com/pnpm/pnpm/issues/11068).
