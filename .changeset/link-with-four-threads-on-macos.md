---
"pacquet": patch
---

Sped up `pnpm install` on macOS. On a 12-core Mac, installing 1352 packages from a warm store took 1.3 seconds, down from 2.2. pnpm now writes package files into `node_modules` with 4 threads on macOS, whatever the number of CPU cores. Set the `PNPM_MACOS_LINK_THREADS` environment variable to use a different number of threads, or to `0` to write files with the threads pnpm uses for the rest of the install.
