---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

When a pnpm-lock.yaml contains two documents, ignore the first one. pnpm v11 will write two lockfile documents into pnpm-lock.yaml in order to store pnpm version integrities and config dependency resolutions.
