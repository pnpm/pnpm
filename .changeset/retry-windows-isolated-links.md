---
"pacquet": patch
---

Retry transient Windows file-lock errors, including sharing violations, while linking dependencies with the default (isolated) `nodeLinker`. This fixes [pnpm/pnpm#14407](https://github.com/pnpm/pnpm/issues/14407).
