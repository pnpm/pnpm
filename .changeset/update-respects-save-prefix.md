---
"pacquet": patch
---

`pnpm update --latest` now applies the `savePrefix` setting when it rewrites a dependency whose range has no operator of its own, such as `<2.0.0`.
