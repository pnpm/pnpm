---
"@pnpm/config": patch
"@pnpm/core": patch
"@pnpm/plugin-commands-installation": patch
"@pnpm/resolve-dependencies": patch
---

When `lockfile-include-tarball-url` is set to `true`, every entry in `pnpm-lock.yaml` will contain the full URL to the package's tarball [#5054](https://github.com/pnpm/pnpm/pull/5054).
