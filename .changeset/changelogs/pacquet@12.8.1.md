## 12.8.1

pnpm 12.8.1 fixes `pnpm install --frozen-lockfile` rejecting lockfiles with injected workspace packages that have peers, restores the executable bit on files of local directory dependencies, makes `pnpm dedupe` converge, and uses less CPU on many-core machines.

### Patch Changes

- `pnpm install --frozen-lockfile` no longer rejects a freshly generated lockfile when an injected workspace package has peer dependencies [#16332](https://github.com/pnpm/pnpm/issues/16332).

- Executable files in a `file:` directory dependency or an injected workspace package keep their executable bit again. Since 12.8.0, pnpm installed these files without the permissions they have in their project.

- `pnpm dedupe` now reaches a stable lockfile when a package's peer suffix is long enough to be hashed. Before, each run could switch that package's key between the hashed and the spelled-out suffix, so `pnpm dedupe --check` always failed [#16331](https://github.com/pnpm/pnpm/issues/16331).

- `pnpm install --frozen-lockfile`, the default in CI, now uses less CPU on machines with more than 8 cores. Warm installs on many-core Windows machines got up to 10% faster. Frozen installs now link with at most 16 worker threads.

- `verifyDepsBeforeRun` no longer reports dependencies as outdated after a filtered install just because `pnpm-lock.yaml` has a newer modification time. It checks the lockfile against the packages that install put in place. Before, `pnpm run` reinstalled the whole workspace with lifecycle scripts on, for example after a Docker `COPY` brought in a lockfile with a newer mtime [#16322](https://github.com/pnpm/pnpm/issues/16322).

  After a filtered install, `verifyDepsBeforeRun` now also checks that the install put the selected projects' dependencies in place. A `node_modules` directory alone no longer counts as proof.

- `pnpm run` and `pnpm exec` no longer install a project that has never been installed and has nothing to install. Such a project declares no dependencies, no peer dependencies that `autoInstallPeers` would fetch, and no install lifecycle scripts. The command now runs without writing `node_modules` or `pnpm-lock.yaml` [#16313](https://github.com/pnpm/pnpm/issues/16313).

- `pnpm update -g --latest` now upgrades globally installed packages beyond their saved version ranges [#16320](https://github.com/pnpm/pnpm/issues/16320).
