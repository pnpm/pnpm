---
"@pnpm/package-requester": major
"@pnpm/store-controller-types": major
---

`RequestPackageOptions` now takes a union type for the `update` option, instead of a separate `updateToLatest` option.

This avoids pitfalls around specifying only `update` or, specifying `update: false`, but still providing `updateToLatest: true`.
