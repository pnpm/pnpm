---
"@pnpm/config.reader": patch
"pnpm": patch
---

Do not resolve `$` override references against a root manifest that has no direct dependencies. This keeps commands such as `pnpm fetch` working when Docker layers include only a lockfile, `pnpm-workspace.yaml`, and a package-manager-only `package.json` [#12160](https://github.com/pnpm/pnpm/issues/12160).
