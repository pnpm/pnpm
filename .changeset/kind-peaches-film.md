---
"@pnpm/hooks.pnpmfile": patch
"pnpm": patch
---

Validate all `readPackage` dependency map fields, including `devDependencies`, and reject falsy non-object invalid values instead of silently accepting them.
