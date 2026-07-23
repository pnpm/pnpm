---
"@pnpm/releasing.commands": minor
"pacquet": minor
"pnpm": minor
---

`pnpm pack --json` now also prints the manifest that goes into the tarball, so `pnpm pack --dry-run --json` previews the published `package.json` — with `publishConfig` overrides applied, `workspace:`/`catalog:`/`jsr:` specifiers replaced, and the publish lifecycle scripts and the `pnpm` field stripped — without writing a tarball.

`pnpm pack` accepts `--ignore-scripts` now, which skips the `prepack`, `prepare`, and `postpack` scripts. Combined with `--dry-run --json`, it prints the manifest without building the package.
