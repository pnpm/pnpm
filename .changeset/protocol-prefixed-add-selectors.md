---
"pacquet": patch
---

`pnpm add` now saves protocol-prefixed selectors such as `jsr:@scope/pkg` under the package name inside the selector instead of treating the protocol prefix as part of the name [pnpm/pnpm#14590](https://github.com/pnpm/pnpm/issues/14590).
