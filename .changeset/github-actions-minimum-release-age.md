---
"@pnpm/deps.github-actions": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---
`pnpm outdated` and `pnpm update` now apply `minimumReleaseAge` to GitHub Actions. `minimumReleaseAgeExclude` entries match action names such as `actions/checkout` [#13923](https://github.com/pnpm/pnpm/issues/13923).
