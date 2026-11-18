---
"pacquet": minor
---

With `trustPolicy: no-downgrade`, `pnpm update --interactive` shows a `Trust` column with the trust evidence of each offered registry dependency [#8889](https://github.com/pnpm/pnpm/issues/8889). The list also offers only versions the trust policy accepts, so it matches what the update installs.
