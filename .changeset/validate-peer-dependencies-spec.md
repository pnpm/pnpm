---
"pacquet": patch
---

`pnpm install` now fails with `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION` when a `peerDependencies` value is neither a version range nor a valid specifier. A typo such as `"foo": "foo@1.0.0"` used to install as a directory link, leaving a broken symlink in `node_modules` [#14791](https://github.com/pnpm/pnpm/issues/14791).
