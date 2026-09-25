---
"@pnpm/lockfile.fs": patch
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

The error for an incompatible pnpm-lock.yaml now reports the lockfileVersion the file was generated with and the lockfileVersion the current pnpm supports. The error also warns that recreating the lockfile with `--force` may break the application and suggests installing the pnpm version that generated the lockfile [#848](https://github.com/pnpm/pnpm/issues/848).
