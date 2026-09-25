---
"@pnpm/releasing.commands": patch
"pacquet": minor
"pnpm": patch
---

`pnpm pack --json` now also reports the manifest that goes into the tarball. `pnpm pack --dry-run --json` previews the published `package.json` without writing a tarball. `publishConfig` overrides are applied, `workspace:`, `catalog:`, and `jsr:` specifiers are replaced, the `pnpm` field is dropped, and publish lifecycle scripts are stripped unless `--skip-manifest-obfuscation` is set.

`pnpm pack` now accepts `--ignore-scripts` to skip the `prepack`, `prepare`, and `postpack` scripts. Combined with `--dry-run --json`, it prints the manifest without building the package.
