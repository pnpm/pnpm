---
"pacquet": patch
---

On Windows, warm `pnpm install --frozen-lockfile` runs are 4-5% faster on 4- and 8-core machines. pnpm now links with one worker thread per core on Windows, between 4 and 16. This changes frozen installs and installs in projects without a `pnpm-workspace.yaml` on machines with 3 to 15 cores.
