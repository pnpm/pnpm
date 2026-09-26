---
"@pnpm/pkg-manifest.utils": patch
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

`pnpm add` now saves the requested exact version when adding a dependency, even when the manifest already contains a version range [#6040](https://github.com/pnpm/pnpm/issues/6040).
