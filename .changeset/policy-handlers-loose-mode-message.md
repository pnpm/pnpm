---
"@pnpm/installing.commands": patch
"pnpm": patch
---

Improve the log message that pnpm prints after auto-adding entries to `minimumReleaseAgeExclude` when `minimumReleaseAge` is set without `minimumReleaseAgeStrict`. The message previously referred to the internal "loose mode" terminology, which wasn't searchable in the docs; it now tells the user to set `minimumReleaseAgeStrict` to `true` if they want to disable this behavior [#11747](https://github.com/pnpm/pnpm/issues/11747).
