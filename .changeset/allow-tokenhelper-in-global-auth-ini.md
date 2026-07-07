---
"@pnpm/config.reader": patch
"pnpm": patch
---

A `tokenHelper` set in the global pnpm `auth.ini` is no longer rejected as project-level configuration. The guard that blocks `tokenHelper` from a project `.npmrc` only treated `~/.npmrc` as a trusted source, so a helper written to `auth.ini` (for example by `pnpm config set`) failed on every command and could not even be removed with `pnpm config delete`. A `tokenHelper` in a workspace or project `.npmrc` is still rejected.
