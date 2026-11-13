## 1100.1.17

### Patch Changes

- `pnpm install --frozen-lockfile` now accepts a lockfile when an injected workspace package declares a `file:` or `link:` dependency with backslashes, such as `file:..\vendor`, or with a slash after the protocol, such as `file:/./vendor`. Before, the install reported the lockfile as out of date right after `pnpm install` had written it [#17720](https://github.com/pnpm/pnpm/pull/17720).

- When a package injected through a path, such as `file:../lib` or `../lib`, gives one of its dependencies a `catalog:` specifier, `pnpm install` now fails the way a fresh resolution does, and `pnpm install --frozen-lockfile` reports the lockfile as outdated. Before, both could accept the existing lockfile [#17722](https://github.com/pnpm/pnpm/pull/17722).

- `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` now accept a `workspace:` dependency on a project whose version is not valid semver, such as `workspace:1` on a project at version `1`. Before, they reported the lockfile that `pnpm install` had just written as out of date.

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.9
  - @pnpm/deps.path@1101.0.9
  - @pnpm/installing.context@1101.0.16
  - @pnpm/installing.deps-resolver@1102.2.13
  - @pnpm/lockfile.types@1100.1.7
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/resolving.git-resolver@1100.1.30
  - @pnpm/resolving.local-resolver@1101.2.9
  - @pnpm/resolving.npm-resolver@1104.2.10
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/workspace.project-manifest-reader@1100.1.6
