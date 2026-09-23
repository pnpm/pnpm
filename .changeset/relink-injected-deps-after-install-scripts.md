---
"pacquet": patch
---

`pnpm install` now copies the output of a workspace package's own `prepare`, `install`, or `postinstall` script into the injected copies of that package. Before, the injected copies kept only the files that existed before the script ran [#9464](https://github.com/pnpm/pnpm/issues/9464).
