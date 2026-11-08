---
"pacquet": minor
---

`pnpm store add` now accepts directories of tarballs, such as `pnpm store add ./tarballs`, and adds every `.tgz`, `.tar.gz`, and `.tar` file inside them recursively. `pnpm store add` is now faster when it adds many packages or local tarballs [#17875](https://github.com/pnpm/pnpm/issues/17875).
