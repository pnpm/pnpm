---
"@pnpm/config": patch
"@pnpm/core": patch
"@pnpm/plugin-commands-installation": patch
"@pnpm/resolve-dependencies": patch
---

(EXPERIMENTAL) When the `use-experimental-npmjs-files-index` option is set to `true` and `--lockfile-only` installation is performed, package tarballs are not downloaded. npm's beta file index feature is used to populate the lockfile [#7117](https://github.com/pnpm/pnpm/pull/7177).
