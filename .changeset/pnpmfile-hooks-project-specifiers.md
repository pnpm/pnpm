---
"pacquet": patch
---

A `readPackage` hook that rewrites one of the project's *own* dependency specifiers is honored again. `"is-positive": "^1.0.0"` rewritten to `1.0.0` by a `.pnpmfile.cjs` resolved against the raw range and recorded `specifier: ^1.0.0` for the importer, where pnpm resolves and records `1.0.0` [#13769](https://github.com/pnpm/pnpm/issues/13769).
