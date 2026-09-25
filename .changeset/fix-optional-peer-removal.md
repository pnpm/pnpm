---
"@pnpm/hooks.read-package-hook": patch
"pnpm": patch
"pacquet": patch
---

Removal overrides such as `"parent>peer": "-"` now prevent optional peers from being installed from another workspace package [#15008](https://github.com/pnpm/pnpm/issues/15008).
