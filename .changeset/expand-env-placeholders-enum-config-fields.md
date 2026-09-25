---
"pacquet": patch
---

A setting in `pnpm-workspace.yaml` may now be written as an environment variable placeholder, such as `nodeLinker: ${PNPM_NODE_LINKER:-isolated}` or `ignoreScripts: ${CI:-false}`. Reading the file used to fail for every setting that does not take free text [#14914](https://github.com/pnpm/pnpm/issues/14914).
