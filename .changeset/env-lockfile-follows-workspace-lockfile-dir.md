---
"pnpm": patch
---

pnpm now records the pinned pnpm version in the `pnpm-lock.yaml` under the `lockfileDir` set in `pnpm-workspace.yaml`. It used to write a second `pnpm-lock.yaml` at the workspace root.
