---
"pacquet": patch
---

`pnpm deploy` now finds patches and local dependencies when the target directory sits under a symlink, such as `/tmp` on macOS. It failed with `ERR_PNPM_PATCH_NOT_FOUND` [#16470](https://github.com/pnpm/pnpm/issues/16470).
