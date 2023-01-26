---
"pnpm": patch
---

Fixed `EMFILE: too many open files` by using graceful-fs for reading bin files of dependencies [#5887](https://github.com/pnpm/pnpm/issues/5887).
