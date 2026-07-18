## 1100.2.11

### Patch Changes

- `peerDependencies` now accept dependency specifiers that carry a scheme — a named-registry spec (`<registry>:<version>`), an `npm:` alias, or a `file:`/git/URL spec — instead of rejecting them with `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION` [#13095](https://github.com/pnpm/pnpm/issues/13095). Such a peer is matched against the semver range carried by the specifier (`work:5.x.x` is checked as `5.x.x`, `npm:bar@^5` as `^5`), or against `*` when it carries no version, while the original specifier still selects the package to auto-install. Bare `name@version` values, which are almost always a mistake, are still rejected.

- Updated dependencies:
  - @pnpm/deps.peer-range@1100.1.0
  - @pnpm/lockfile.preferred-versions@1100.0.22
  - @pnpm/pkg-manifest.utils@1100.2.9
  - @pnpm/resolving.npm-resolver@1102.1.5
