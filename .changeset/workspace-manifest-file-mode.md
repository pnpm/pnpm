---
"pacquet": patch
---

`pnpm deploy` now writes `package.json`, `pnpm-lock.yaml`, and `pnpm-workspace.yaml` with the mode the umask gives a new file, such as `0644`. A new `pnpm-workspace.yaml` gets the same mode, and an edit keeps the mode the file already had.
