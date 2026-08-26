---
"pacquet": patch
---

A path named by the `pnpmfile` setting that is not on disk now fails with `ERR_PNPM_PNPMFILE_NOT_FOUND` and names the file, instead of surfacing as a generic pnpmfile execution failure. Discovery of the default `.pnpmfile.mjs` / `.pnpmfile.cjs` is unaffected: a project that ships neither still installs normally.
