---
"pacquet": patch
---

`pnpm install` and `pnpm peers check` no longer report an unmet peer when a workspace package's peer is provided by a remote tarball URL. pnpm now compares the peer range with the version in the tarball's manifest [#17037](https://github.com/pnpm/pnpm/issues/17037).
