---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` now match `.npmignore` and `.gitignore` rules the way npm does. A negation such as `!lib/**` or `!lib/**/!(*.map)` re-includes files under a directory that an earlier `*` rule excluded [#16743](https://github.com/pnpm/pnpm/issues/16743).
