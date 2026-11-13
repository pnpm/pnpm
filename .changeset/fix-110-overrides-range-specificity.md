---
"@pnpm/hooks.read-package-hook": patch
"pnpm": patch
"pacquet": patch
---

When multiple `overrides` selectors match a dependency, pnpm now picks the selector with the narrower version range regardless of the order in which the overrides are declared [#18071](https://github.com/pnpm/pnpm/pull/18071).
