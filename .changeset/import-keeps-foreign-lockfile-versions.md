---
"pacquet": minor
---

`pnpm import` now prefers the versions recorded in `package-lock.json`, `npm-shrinkwrap.json`, or `yarn.lock` when it generates `pnpm-lock.yaml`. It resolved every dependency from scratch before, so the imported lockfile could pin newer versions than the one it was generated from [#14476](https://github.com/pnpm/pnpm/issues/14476). An imported version is a preference, not a constraint, so a range in `package.json`, a catalog, or an override still decides which versions are eligible. This matches pnpm 11.

`pnpm import` in a workspace now imports every workspace project, as pnpm 11 does. It previously imported only the project in the current directory.

`pnpm import` fails with `ERR_PNPM_LOCKFILE_NOT_FOUND` when none of the three source lockfiles are present, and with `ERR_PNPM_YARN_LOCKFILE_PARSE_FAILED` when it cannot parse `yarn.lock`. It used to generate a lockfile from scratch in both cases.

`pnpm import` always resolves locally and no longer offloads resolution to a pnpr server. It warns when `--pnpr-server` or the `pnpr-server` setting is given.
