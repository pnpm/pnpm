---
"@pnpm/config.reader": major
"pnpm": minor
---

`pnpm dlx` and `pnpm create` now respect security and trust policy settings (`minimumReleaseAge`, `minimumReleaseAgeExclude`, `minimumReleaseAgeStrict`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`) from project-level configuration [#11183](https://github.com/pnpm/pnpm/issues/11183).
