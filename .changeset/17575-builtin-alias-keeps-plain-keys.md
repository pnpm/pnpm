---
"@pnpm/config.pick-registry-for-package": patch
"@pnpm/deps.status": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/lockfile.utils": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/workspace.state": patch
"pnpm": patch
"pacquet": patch
---

Packages from the default registry keep plain lockfile keys, even when a prefix such as `npmjs` points at the registry set as `registry`. A dependency written with such a prefix shares the plain entry of the same package. The next install rewrites keys that earlier versions qualified this way [#17575](https://github.com/pnpm/pnpm/issues/17575).
