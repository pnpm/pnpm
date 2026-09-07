## 1100.2.6

### Patch Changes

- Fixed `pnpm install` failing with `ERR_PNPM_LOCKFILE_IS_SYMLINK` in a project with config dependencies when `pnpm-lock.yaml` is a symlink, as build sandboxes such as Bazel and Nix stage it. pnpm no longer rewrites the lockfile when the recorded config dependencies are unchanged. Writing changed config dependencies through a symlinked lockfile is still refused. A lockfile that starts with a byte order mark now keeps its main document when its config dependencies are updated [#14372](https://github.com/pnpm/pnpm/issues/14372).

- `pnpm install` no longer lets a symlink swapped into the path of `pnpm-lock.yaml` during a config dependency update redirect the lockfile write to the symlink target [pnpm/pnpm#14322](https://github.com/pnpm/pnpm/issues/14322).

- `pnpm install` now relinks workspace packages when `publishConfig.linkDirectory` changes. Frozen installs report an outdated lockfile until it is regenerated [pnpm/pnpm#14488](https://github.com/pnpm/pnpm/issues/14488).

- Updated dependencies:
  - @pnpm/error@1100.1.4
  - @pnpm/lockfile.merger@1100.0.22
  - @pnpm/lockfile.types@1100.1.1
  - @pnpm/lockfile.utils@1102.1.1
