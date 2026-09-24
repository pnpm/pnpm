---
"pacquet": patch
---

`pnpm install` uses less CPU when it links packages from a warm store. pnpm now links the files of one directory from one worker, one after another, and links different directories in parallel. Before, every worker linked into the same directory at once and the workers queued on the directory lock, which on Windows made a warm install several times slower than on pnpm 11 [#15439](https://github.com/pnpm/pnpm/issues/15439).
