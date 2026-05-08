---
"@pnpm/cli.parse-cli-args": patch
"pnpm": patch
---

Fixed `pnpm --prefix=<dir> install` overwriting the existing `pnpm-workspace.yaml` in `<dir>` with `set this to true or false` placeholders. The renamed `--prefix` option (which maps to `dir`) was not honored when locating the workspace root, so the workspace manifest's `allowBuilds` settings were not loaded into config and got clobbered when ignored builds were auto-populated [#11535](https://github.com/pnpm/pnpm/issues/11535).
