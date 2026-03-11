---
"pnpm": patch
---

Fix `pnpm publish` for scoped packages returning 404 from the npm registry.

The underlying issue was that `npm-package-arg@13.0.2` used lowercase `%2f` when URL-encoding the slash in scoped package names. For example, `@scope/package` was encoded as `@scope%2f<name>` instead of `@scope%2F<name>`. The npm registry requires uppercase encoding and returns 404 for the lowercase variant.

Patched `npm-package-arg@13.0.2` to use uppercase `%2F` as documented in its own code comment.

Fixes #9621.
