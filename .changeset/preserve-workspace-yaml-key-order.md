---
"@pnpm/workspace.workspace-manifest-writer": patch
"pnpm": patch
---

Preserve the original key order in `pnpm-workspace.yaml` when updating it. Existing keys keep their position, and new keys are inserted in alphabetical position when the existing keys are already sorted (with a leading `packages` key allowed) or appended at the end otherwise.
