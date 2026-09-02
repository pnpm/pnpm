---
"pacquet": patch
---

Retry transient Windows file-lock errors while linking dependencies with the default (isolated) `nodeLinker`, and treat sharing violations as transient locks in every retried filesystem operation. This fixes [pnpm/pnpm#14407](https://github.com/pnpm/pnpm/issues/14407).
