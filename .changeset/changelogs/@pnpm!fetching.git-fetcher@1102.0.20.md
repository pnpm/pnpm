## 1102.0.20

### Patch Changes

- `pnpm install` no longer appears to hang when a git dependency is fetched over SSH and ssh asks for a key passphrase or a host key confirmation. pnpm now runs ssh in batch mode, so the install fails right away with the ssh error, and a key that needs a passphrase has to be loaded into an SSH agent first. An ssh command selected through `GIT_SSH_COMMAND`, `GIT_SSH`, or the `core.sshCommand` git setting is kept as is [#2227](https://github.com/pnpm/pnpm/issues/2227).

- `pnpm install` now fetches committed submodules of git dependencies [pnpm/pnpm#1470](https://github.com/pnpm/pnpm/issues/1470).

- Updated dependencies:
  - @pnpm/error@1100.2.0
  - @pnpm/exec.prepare-package@1100.0.38
  - @pnpm/fetching.fetcher-base@1100.2.11
  - @pnpm/fs.packlist@1100.0.5
  - @pnpm/network.git-utils@1100.0.5
  - @pnpm/resolving.git-resolver@1100.1.23
  - @pnpm/store.index@1100.3.2
