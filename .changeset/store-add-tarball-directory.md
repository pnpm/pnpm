---
"pacquet": minor
---

`pnpm store add` now accepts directories of tarballs, such as `pnpm store add ./tarballs`, and adds every `.tgz`, `.tar.gz`, and `.tar` file inside them recursively. It also ingests packages and local tarballs concurrently and reads each local tarball once [#17875](https://github.com/pnpm/pnpm/issues/17875).
