## 11.28.2

pnpm 11.28.2 fixes `pnpm install` skipping every workspace project whose common ancestor is the filesystem root, and stops `pnpm run` from reinstalling or installing when nothing needs it.

### Patch Changes

- `pnpm install` reported success without installing anything when the workspace projects' common ancestor was the filesystem root, such as `/` or a drive root like `C:\`. It now installs these projects [#16328](https://github.com/pnpm/pnpm/issues/16328).

- `verifyDepsBeforeRun` no longer reports dependencies as outdated after a filtered install just because `pnpm-lock.yaml` has a newer modification time. It checks the lockfile against the packages that install put in place. Before, `pnpm run` reinstalled the whole workspace with lifecycle scripts on, for example after a Docker `COPY` brought in a lockfile with a newer mtime [#16322](https://github.com/pnpm/pnpm/issues/16322).

  After a filtered install, `verifyDepsBeforeRun` now also checks that the install put the selected projects' dependencies in place. A `node_modules` directory alone no longer counts as proof.

- `pnpm run` and `pnpm exec` no longer install a project that has never been installed and has nothing to install. Such a project declares no dependencies, no peer dependencies that `autoInstallPeers` would fetch, and no install lifecycle scripts. The command now runs without writing `node_modules` or `pnpm-lock.yaml` [#16313](https://github.com/pnpm/pnpm/issues/16313).
