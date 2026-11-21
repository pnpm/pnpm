---
"pacquet": patch
"get-pnpm": patch
---

When the `pnpm` package downloads its native binary on first run, a `${NPM_TOKEN?}` placeholder in `.npmrc` now expands to the variable. It expanded to an empty value, so the download was sent without credentials.
