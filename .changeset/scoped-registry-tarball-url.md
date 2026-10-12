---
"pacquet": patch
---

`pnpm-lock.yaml` no longer records the tarball URL of a package installed from a scoped registry, such as one set by `@types:registry=`, when pnpm can rebuild the URL from that registry [#16866](https://github.com/pnpm/pnpm/issues/16866).
