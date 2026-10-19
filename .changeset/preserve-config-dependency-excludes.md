---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` now keep exclusions for resolved configuration dependencies and their transitive dependencies [pnpm/pnpm#17142](https://github.com/pnpm/pnpm/issues/17142).
