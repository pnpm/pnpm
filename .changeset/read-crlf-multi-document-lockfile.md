---
"pacquet": patch
---

Fixed `pnpm install` ignoring a `pnpm-lock.yaml` that carries a leading env lockfile document when the file has CRLF line endings or a UTF-8 byte order mark, as a `core.autocrlf` checkout on Windows produces. The lockfile was reported as broken with `multiple YAML documents detected` and every dependency was re-resolved from the registry [#13606](https://github.com/pnpm/pnpm/issues/13606).
