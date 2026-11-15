## 1101.2.9

### Patch Changes

- `pnpm install --frozen-lockfile` now accepts a lockfile when an injected workspace package declares a `file:` or `link:` dependency with backslashes, such as `file:..\vendor`, or with a slash after the protocol, such as `file:/./vendor`. Before, the install reported the lockfile as out of date right after `pnpm install` had written it [#17720](https://github.com/pnpm/pnpm/pull/17720).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.9
  - @pnpm/deps.path@1101.0.9
  - @pnpm/resolving.resolver-base@1101.3.5
  - @pnpm/workspace.project-manifest-reader@1100.1.6
