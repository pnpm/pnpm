---
"@pnpm/building.after-install": patch
"pnpm": patch
---

Security: `pnpm rebuild` now refuses a lockfile whose `packages` key carries a path traversal in the package name (e.g. `../../../escaped@1.0.0`), instead of running that package's lifecycle scripts and linking its bins in a directory outside the virtual store. Such a name is rejected with `ERR_PNPM_INVALID_DEPENDENCY_NAME`.
