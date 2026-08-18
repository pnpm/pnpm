---
"pacquet": patch
---

A `.pnpmfile.cjs` `readPackage` hook that rewrites one of a project's *own* dependency specifiers is now honored: rewriting `"is-positive": "^1.0.0"` to `1.0.0` installs 1.0.0 and records `specifier: 1.0.0` for the importer. Previously the hook was applied only to the manifests of resolved dependencies, so a project's own specifier resolved against the raw range from `package.json` [#13769](https://github.com/pnpm/pnpm/issues/13769).
