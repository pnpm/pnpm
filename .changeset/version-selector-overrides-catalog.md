---
"@pnpm/deps.inspection.outdated": patch
"@pnpm/deps.status": patch
"@pnpm/hooks.read-package-hook": patch
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Version-selector `overrides` entries such as `"ms@^2.0.0": "2.1.1"` now apply to dependencies declared with the `catalog:` protocol when the catalog entry's range matches the selector [#17922](https://github.com/pnpm/pnpm/issues/17922).
