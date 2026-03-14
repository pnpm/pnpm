---
"@pnpm/plugin-commands-audit": patch
"pnpm": patch
---

Fix `pnpm audit` ignored vulnerability counts to include all ignored occurrences in dependency paths.

Previously, ignored advisories were counted once per advisory, which could under-report ignored counts when a single advisory appeared in multiple paths. The summary now reports ignored counts based on all affected paths.

Fixes #10646
