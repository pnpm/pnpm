---
"pacquet": patch
---

A `package.json` that starts with a UTF-8 byte order mark is read again instead of failing with `expected value at line 1 column 1` — workspace discovery, dependency manifests, and manifests bundled in a tarball all accept one, matching pnpm [#13311](https://github.com/pnpm/pnpm/issues/13311). A manifest that really is malformed now reports its path in the error.
