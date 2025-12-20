---
"@pnpm/plugin-commands-listing": patch
"@pnpm/reviewing.dependencies-hierarchy": patch
"@pnpm/types": patch
"@pnpm/list": patch
"pnpm": patch
---

`pnpm list` and `pnpm why` now display npm: protocol for aliased packages (e.g., `foo npm:is-odd@3.0.1`) [#8660](https://github.com/pnpm/pnpm/issues/8660).
