---
"pacquet": minor
---

With `trustPolicy: no-downgrade`, `pnpm update --interactive` shows a `Trust` column with the trust evidence of each offered version. A version with weaker evidence than the installed one is shown in red, next to the evidence it loses [#8889](https://github.com/pnpm/pnpm/issues/8889).
