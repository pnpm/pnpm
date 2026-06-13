---
"@pnpm/installing.commands": patch
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
---

Fix garbled summary line after submitting `pnpm update -i` and `pnpm audit --fix -i`. The interactive checkbox prompt previously printed every selected choice's full table row (label, current/target versions, workspace, URL) joined by commas, producing a wall of text after pressing Enter. The summary now lists only the selected package names (or vulnerability keys) by setting an explicit `short` per choice; the in-progress selection UI is unchanged.
