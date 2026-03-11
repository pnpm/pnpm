---
"pnpm": patch
---

Fix `pnpm publish` for scoped packages returning 404 from the npm registry.

The underlying issue was that `npm-package-arg@13.0.2` used lowercase `%2f` when URL-encoding the slash in scoped package names (e.g. `@scope%2fpackage`), but the npm registry requires uppercase `%2F` (e.g. `@scope%2Fpackage`).

Patched `npm-package-arg@13.0.2` to use uppercase `%2F` as documented in its own code comment.

Fixes #9621.
