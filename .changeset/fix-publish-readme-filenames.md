---
"@pnpm/releasing.exportable-manifest": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm publish` omitting README metadata when the file is named `README` or `readme.markdown`, including when publishing a tarball [pnpm/pnpm#12704](https://github.com/pnpm/pnpm/issues/12704).
