---
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

Fixed a crash when streaming lifecycle script output (e.g. with `--stream`). The reporter no longer passes an infinite width to `cli-truncate`, which the latest version rejects.
