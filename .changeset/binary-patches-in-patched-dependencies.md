---
"pnpm": patch
---

`pnpm patch-commit` now records changes to binary files, and `patchedDependencies` patches that change binary files apply byte for byte. Before, binary files were written to the patch as mangled text diffs [#18430](https://github.com/pnpm/pnpm/pull/18430).
