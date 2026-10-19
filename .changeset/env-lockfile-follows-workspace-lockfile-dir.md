---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now records the pinned pnpm version in the `pnpm-lock.yaml` under the `lockfileDir` set in `pnpm-workspace.yaml`, and resolves a relative `lockfileDir` from the workspace root. pnpm used to write a second `pnpm-lock.yaml` at the workspace root.
