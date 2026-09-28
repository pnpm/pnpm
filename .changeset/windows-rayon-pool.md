---
"pacquet": patch
---

`pnpm install` on Windows is faster on machines with 4 to 8 cores. On a 4-core machine, a fresh install of a project with 1,352 packages took 3.9 s instead of 4.5 s. pnpm now links with one worker thread per core on Windows, between 4 and 16.
