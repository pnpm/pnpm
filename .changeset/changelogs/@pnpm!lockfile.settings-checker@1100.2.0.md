## 1100.2.0

### Minor Changes

- Changing `autoInstallPeers`, `dedupePeers`, `peersSuffixMaxLength`, `excludeLinksFromLockfile`, or `injectWorkspacePackages` no longer re-resolves the dependency graph when the lockfile proves the setting cannot affect it: no package or project declares a peer dependency for the peer settings, and no project depends on a directory or on another workspace project for the link and injection settings. The new setting is recorded in `pnpm-lock.yaml` and the install proceeds from the existing resolution. Every other case still falls back to a full resolution.

### Patch Changes

- Checking whether `ignoredOptionalDependencies` is up to date no longer reorders the configured patterns. The check sorted them in place, which could move an `!` exclusion ahead of the pattern it excludes from and flip which optional dependencies were ignored.

- Updated dependencies:
  - @pnpm/lockfile.verification@1100.0.32
