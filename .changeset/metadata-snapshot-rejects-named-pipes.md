---
"pacquet": patch
---

`pnpm install` and `pnpm add` now fail with an error when a file they snapshot before the install, such as `package.json`, `pnpm-lock.yaml` or `pyproject.toml`, is a named pipe or a device. The command used to wait forever for something to write to it.
