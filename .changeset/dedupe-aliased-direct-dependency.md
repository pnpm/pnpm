---
"pacquet": patch
---

`pnpm dedupe` no longer alternates between two lockfiles when a dependency's range matches both a direct dependency and an `npm:` alias of the same package. Repeated runs now settle on the version of the direct dependency [#15588](https://github.com/pnpm/pnpm/issues/15588).
