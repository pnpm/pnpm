## 12.8.1

### Patch Changes

- `pnpm install --frozen-lockfile`, the default in CI, now uses less CPU on machines with more than 8 cores. Warm installs on many-core Windows machines got up to 10% faster. Frozen installs now link with at most 16 worker threads.

- `pnpm dedupe` now reaches a stable lockfile when a package's peer suffix is long enough to be hashed. Before, each run could switch that package's key between the hashed and the spelled-out suffix, so `pnpm dedupe --check` always failed [#16331](https://github.com/pnpm/pnpm/issues/16331).

- `pnpm install --frozen-lockfile` no longer rejects a freshly generated lockfile when an injected workspace package has peer dependencies [#16332](https://github.com/pnpm/pnpm/issues/16332).

- `pnpm update -g --latest` now upgrades globally installed packages beyond their saved version ranges [#16320](https://github.com/pnpm/pnpm/issues/16320).

- Files of a `file:` directory dependency or an injected workspace package now keep the permissions they have in their project. Since 12.8.0, an executable file in such a dependency was installed without its executable bit.

- `pnpm run` and `pnpm exec` no longer install a project that has never been installed and has nothing to install. Such a project declares no dependencies, no peer dependencies that `autoInstallPeers` would fetch, and no install lifecycle scripts. The command now runs without writing `node_modules` or `pnpm-lock.yaml` [pnpm/pnpm#16313](https://github.com/pnpm/pnpm/issues/16313).

- `verifyDepsBeforeRun` no longer reports dependencies as outdated after a filtered install just because `pnpm-lock.yaml` has a newer modification time. It checks the lockfile against the packages that install put in place. Before, `pnpm run` reinstalled the whole workspace with lifecycle scripts on, for example after a Docker `COPY` brought in a lockfile with a newer mtime [#16322](https://github.com/pnpm/pnpm/issues/16322).

  After a filtered install, `verifyDepsBeforeRun` now also checks that the install put the selected projects' dependencies in place. A `node_modules` directory alone no longer counts as proof.
