---
"@pnpm/deps.github-actions": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm update` and `pnpm outdated` with `--include-github-actions` matching every package dependency when combining a GitHub Action selector with a negated package selector such as `!lodash`.
