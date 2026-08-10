---
"pacquet": patch
---

On Windows, installation no longer fails with "A required privilege is not held by the client. (os error 1314)" when symlink creation requires elevation (e.g. Developer Mode is off) — pnpm now falls back to NTFS junctions in that case. Additionally, `pnpm clean` and `pnpm deploy --force` no longer fail with "Access is denied. (os error 5)" when removing the package links inside `node_modules` [#13694](https://github.com/pnpm/pnpm/issues/13694).
