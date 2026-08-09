## 1102.0.16

### Patch Changes

- The automatic `packageManager` version switch works again on registries whose tarball URLs point at a different host than the registry itself (load-balanced feed proxies, Artifactory-style mirrors). Package-manager entries are now always recorded with integrity-only resolutions — the download URL is derived from the trusted bootstrap registry instead — and entries persisted in an invalid shape by an earlier pnpm are discarded and re-resolved instead of failing every command [#13619](https://github.com/pnpm/pnpm/issues/13619).

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.2.16
  - @pnpm/installing.deps-resolver@1101.1.1
  - @pnpm/lockfile.fs@1100.2.1
  - @pnpm/resolving.npm-resolver@1103.2.0
