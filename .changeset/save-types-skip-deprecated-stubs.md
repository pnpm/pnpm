---
"pacquet": patch
---

`pnpm add --save-types` no longer adds a `@types/*` package whose resolved version is deprecated. DefinitelyTyped publishes such stubs for packages that ship their own types, such as `@types/typescript` for `typescript` [#15636](https://github.com/pnpm/pnpm/issues/15636).
