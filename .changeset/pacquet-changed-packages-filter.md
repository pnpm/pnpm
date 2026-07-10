---
"pnpm": patch
---

The native pnpm CLI now supports the changed-packages filter selector (`--filter "...[<since>]"`), selecting the workspace projects whose files changed since the given git ref. The `testPattern` and `changedFilesIgnorePattern` settings (and the `--test-pattern` / `--changed-files-ignore-pattern` flags) are honored, matching the TypeScript CLI.
