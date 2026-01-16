---
"@pnpm/workspace.manifest-writer": patch
"pnpm": patch
---

Fix YAML formatting preservation in `pnpm-workspace.yaml` when running commands like `pnpm update`. Previously, quotes and other formatting were lost even when catalog values didn't change.

Closes #10425
