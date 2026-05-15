---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

Fix lockfile parsing failures when `pnpm-lock.yaml` contains CRLF line endings and multiple YAML documents [#11612](https://github.com/pnpm/pnpm/issues/11612).
