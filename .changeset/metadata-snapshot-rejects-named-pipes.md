---
"pacquet": patch
---

`pnpm install` and `pnpm add` now report an error when `package.json`, `pnpm-lock.yaml`, `pyproject.toml` or another file they snapshot before installing is a named pipe or a device. The command used to wait forever for something to write to it.
