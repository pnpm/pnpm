## 1100.1.26

### Patch Changes

- `verifyDepsBeforeRun` no longer reports dependencies as outdated after a filtered install just because `pnpm-lock.yaml` has a newer modification time. It checks the lockfile against the packages that install put in place. Before, `pnpm run` reinstalled the whole workspace with lifecycle scripts on, for example after a Docker `COPY` brought in a lockfile with a newer mtime [#16322](https://github.com/pnpm/pnpm/issues/16322).

  After a filtered install, `verifyDepsBeforeRun` now also checks that the install put the selected projects' dependencies in place. A `node_modules` directory alone no longer counts as proof.
