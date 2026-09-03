---
"pacquet": minor
---

`pnpm import` now keeps the versions recorded in `package-lock.json`, `npm-shrinkwrap.json`, or `yarn.lock` when it generates `pnpm-lock.yaml`. A range in `package.json`, a catalog, or an override still decides which versions are eligible, and the recorded version is preferred among them. The generated lockfile previously could pin newer versions than the source lockfile [#14476](https://github.com/pnpm/pnpm/issues/14476).

`pnpm import` in a workspace now imports every workspace project into the shared lockfile. It previously imported only the project in the current directory.

`pnpm import` now fails with `ERR_PNPM_LOCKFILE_NOT_FOUND` when none of the three source lockfiles is present. It also fails with `ERR_PNPM_YARN_LOCKFILE_PARSE_FAILED` when it cannot parse `yarn.lock`. It previously generated a lockfile from scratch in both cases.

`pnpm import` always resolves locally. It warns when `--pnpr-server` or the `pnpr-server` setting is given and does not use the server.
