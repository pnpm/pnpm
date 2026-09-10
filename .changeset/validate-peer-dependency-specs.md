---
"pacquet": patch
---

`pnpm install` now fails with `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION` when a `peerDependencies` value is not a version range, a `workspace:`/`catalog:` spec, or a dependency specifier. pnpm accepted a typo such as `"foo": "foo@1.2.3"` and linked it into `node_modules` as a path that does not exist [#14791](https://github.com/pnpm/pnpm/issues/14791).
