---
"@pnpm/deps.github-actions": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm outdated` and `pnpm update` now apply `minimumReleaseAge` to GitHub Actions. A version whose tag is younger than the minimum release age is not offered, and `minimumReleaseAgeExclude` entries match action names such as `actions/checkout`. The age of an action version comes from its git tag or commit date, which the tag's author sets [#13923](https://github.com/pnpm/pnpm/issues/13923).
