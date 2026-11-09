---
"@pnpm/lockfile.fs": patch
"pnpm": patch
---

pnpm now reads the boot document of a `pnpm-lock.yaml` written in the lockfile format planned for pnpm 13, so it can switch to the pnpm version such a project pins.
