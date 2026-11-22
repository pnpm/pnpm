## 1100.1.44

### Patch Changes

- `pnpm outdated` now reports newer prereleases of an installed prerelease. It checks the dist-tag that points to the newest version of the installed prerelease channel, such as `next` for `3.0.0-rc.0`, alongside `latest` [#7339](https://github.com/pnpm/pnpm/issues/7339).

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.9
  - @pnpm/config.version-policy@1100.2.12
  - @pnpm/deps.path@1101.0.11
  - @pnpm/error@1100.2.3
  - @pnpm/hooks.read-package-hook@1100.3.14
  - @pnpm/installing.client@1100.3.23
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/lockfile.utils@1102.1.13
  - @pnpm/pkg-manifest.utils@1100.4.14
  - @pnpm/resolving.npm-resolver@1104.2.12
  - @pnpm/types@1102.1.5
