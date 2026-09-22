---
"@pnpm/pkg-manifest.utils": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update` now keeps a version range whose shape has no save prefix, such as `<= 3.0.0` or `>=1.0.0 <2.0.0`, when the updated version still satisfies it. Before, the range was replaced with the default prefix, so `<= 3.0.0` became `^3.0.0` and later updates moved past the bound [#6714](https://github.com/pnpm/pnpm/issues/6714).
