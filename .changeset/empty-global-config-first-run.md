---
"pacquet": patch
"get-pnpm": patch
---

When the `pnpm` package downloads its native binary on first run, it now reads a global `config.yaml` that holds only blank lines or comments as no settings. Such a file made every command fail with "expected a document, but the input is empty" [#18172](https://github.com/pnpm/pnpm/issues/18172).
