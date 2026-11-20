---
"@pnpm/releasing.commands": patch
"pacquet": minor
"pnpm": patch
---

`pnpm pack --json` now reports the `package.json` that goes into the tarball, with `publishConfig` and the other publish-time changes applied. Run `pnpm pack --dry-run --json` to preview it without writing a tarball. A dry run still runs `prepack` and `prepare`, unless `--ignore-scripts` is passed.
