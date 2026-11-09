---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

pnpm now reads the boot document of `pnpm-lock.yaml` when it is written in lockfile format 13. pnpm then switches to the pnpm version such a project pins.
