---
"@pnpm/yaml.document-sync": patch
"@pnpm/workspace.workspace-manifest-writer": patch
"pnpm": patch
"pacquet": patch
---

pnpm now preserves scalar YAML anchors and aliases when editing `pnpm-workspace.yaml`. Removing the entry that defines an anchor keeps surviving aliases valid. Entries updated to different values are written separately [#8245](https://github.com/pnpm/pnpm/issues/8245).
