---
"pacquet": patch
---

Fixed `pnpm list -g` with `--depth` greater than `0` reading the current project's lockfile when run inside a project directory.
