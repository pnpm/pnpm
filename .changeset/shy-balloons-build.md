---
"@pnpm/plugin-commands-installation": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/get-context": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

Added support for `pnpm-lock.yaml` format v6. This new format will be the new lockfile format in pnpm v8. To use the new lockfile format, use the `use-lockfile-v6=true` setting in `.npmrc`. Or run `pnpm install --use-lockfile-v6` [#5810](https://github.com/pnpm/pnpm/pull/5810).
