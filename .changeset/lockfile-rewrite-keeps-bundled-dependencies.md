---
"pacquet": patch
---

`bundledDependencies` is no longer dropped from `pnpm-lock.yaml` when an install rewrites it. Bumping a single dependency stripped the field from every unrelated entry that carried it, and a `libc` recorded as a plain string was rewritten as a list [#14153](https://github.com/pnpm/pnpm/issues/14153).
