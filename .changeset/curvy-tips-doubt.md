---
"@pnpm/which-version-is-pinned": patch
"pnpm": patch
---

When updating dependencies, preserve the range prefix in aliased dependencies. So `npm:foo@1.0.0` becomes `npm:foo@1.1.0`.
