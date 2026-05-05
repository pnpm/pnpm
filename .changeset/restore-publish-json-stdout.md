---
"@pnpm/releasing.commands": patch
pnpm: patch
---

Restore npm-CLI-compatible `--json` stdout output for `pnpm publish` ([#11476](https://github.com/pnpm/pnpm/issues/11476)). pnpm 11 reimplemented publish natively ([#10591](https://github.com/pnpm/pnpm/pull/10591)) and inadvertently dropped the per-package JSON object that pnpm 10 emitted transitively via the npm CLI, silently breaking downstream tooling — most notably `nx release publish`, which parses stdout JSON to confirm success ([nrwl/nx#35575](https://github.com/nrwl/nx/issues/35575)). On success, the output is now:

- `pnpm publish --json` → single object `{ id, name, version, size, unpackedSize, shasum, integrity, filename, files, entryCount, bundled }`, mirroring `npm publish --json`.
- `pnpm publish -r --json` → array of those objects, mirroring `pnpm pack --json`'s shape choice.
- `pnpm publish -r --report-summary` → existing `pnpm-publish-summary.json` envelope `{ publishedPackages: [...] }` is preserved, but each entry is upgraded to the same per-package shape (additive — `name` and `version` are still present).
