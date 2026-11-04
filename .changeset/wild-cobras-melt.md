---
"pacquet": patch
---

Commands that update `pnpm-workspace.yaml` now reject a standalone `null` value without changing the file. Empty and comment-only files remain supported.
