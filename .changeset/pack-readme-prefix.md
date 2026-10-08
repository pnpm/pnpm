---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` no longer always include root files that merely start with `README`, `LICENSE`, or `LICENCE`, such as `README_INTERNAL.md`. Only `README`, `LICENSE`, `LICENCE`, and `COPYING`, with or without an extension, ship regardless of `files` and `.npmignore`, as in npm [#16753](https://github.com/pnpm/pnpm/issues/16753).
