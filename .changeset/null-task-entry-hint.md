---
"@pnpm/config.reader": patch
"pnpm": patch
---

A `tasks` entry with no value in `pnpm-workspace.yaml`, such as `build:`, now fails with a hint. The hint suggests writing `{}` for a task with no settings, or removing the entry to keep the default `dependsOn: ['^<task>']`.
