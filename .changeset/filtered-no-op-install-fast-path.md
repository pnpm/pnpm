---
"pacquet": patch
---

A `pnpm install --filter <selector>` run that has nothing to do now reports "Already up to date" without entering the install pipeline, the same way an unfiltered `pnpm install` already did [#14033](https://github.com/pnpm/pnpm/issues/14033).
