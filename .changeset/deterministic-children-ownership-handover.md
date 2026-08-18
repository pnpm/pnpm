---
"pacquet": patch
---

Fixed `pnpm install` writing a different `pnpm-lock.yaml` for an unchanged project depending on the order its dependencies happened to resolve in, which showed up as spurious lockfile diffs between installs.
