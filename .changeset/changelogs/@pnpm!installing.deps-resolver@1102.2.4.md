## 1102.2.4

### Patch Changes

- With `autoInstallPeers`, `pnpm add` and `pnpm remove` in a workspace project keep the locked version of a peer dependency the project declares. In a workspace where another project depended on a different version of that package, the peer could switch to that version [#11225](https://github.com/pnpm/pnpm/issues/11225).

- The global virtual store and the side-effects cache now key built packages by the Node.js version that the root project's `devEngines.runtime` or `engines.runtime` pins. That is the Node.js their build scripts run with. A dependency that declares its own `engines.runtime` no longer changes the key for every other package.

- Adding a dependency now keeps unrelated transitive dependencies on their locked versions [#11456](https://github.com/pnpm/pnpm/issues/11456).

- `pnpm import` in a workspace now keeps the versions pinned by a `yarn.lock` inside a workspace project [#4385](https://github.com/pnpm/pnpm/issues/4385).

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- An optional peer dependency is no longer resolved from another workspace project's package when the project provides one of that package's own peers at a version it rejects. This avoids bogus unmet peer errors [#13989](https://github.com/pnpm/pnpm/issues/13989).

- Do not report unmet peer dependency warnings for aliased `npm:` peer ranges when they are satisfied by a tarball dependency [#11126](https://github.com/pnpm/pnpm/issues/11126).

- Fixed a peer dependency resolving to two different versions for one package. This happened when the package peer-depends on another package and on one of that package's peers, and it is installed deeper than a direct dependency of the package that provides them [#12098](https://github.com/pnpm/pnpm/issues/12098).

- With `resolutionMode: time-based` and `minimumReleaseAge` both set, `pnpm install` no longer reports a subdependency as too new when only the time-based cutoff excludes it. Such subdependencies used to fail a strict install with `ERR_PNPM_NO_MATURE_MATCHING_VERSION`, or were added to `minimumReleaseAgeExclude` [#13569](https://github.com/pnpm/pnpm/issues/13569).

- With `resolutionMode: time-based`, a transitive dependency that has no matching version published before the time-based cutoff now resolves to the lowest matching version allowed by `minimumReleaseAge`. pnpm picks a version younger than `minimumReleaseAge` only if no older version matches [#16298](https://github.com/pnpm/pnpm/issues/16298).

- `pnpm update --recursive <pkg>` no longer changes the version of a peer dependency that another workspace project installs automatically. Such a peer could move to a version outside the range the project declares, for example to React 19 in a project that declares `react: ^18.3.1` [#14928](https://github.com/pnpm/pnpm/issues/14928).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/catalogs.resolver@1100.1.1
  - @pnpm/config.version-policy@1100.2.5
  - @pnpm/deps.graph-hasher@1100.3.5
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/fetching.pick-fetcher@1100.1.13
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/fs.symlink-dependency@1100.0.22
  - @pnpm/hooks.types@1101.0.5
  - @pnpm/lockfile.preferred-versions@1100.0.36
  - @pnpm/lockfile.pruner@1100.0.26
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/patching.config@1100.1.8
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/pkg-manifest.utils@1100.4.7
  - @pnpm/resolving.npm-resolver@1104.2.2
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/store.controller-types@1101.3.2
