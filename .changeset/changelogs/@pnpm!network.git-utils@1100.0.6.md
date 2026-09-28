## 1100.0.6

### Patch Changes

- `pnpm install --frozen-lockfile` now works on a detached HEAD when `gitBranchLockfile` is enabled. The install now reads the lockfiles of the local and remote-tracking branches that contain the checked-out commit. It still writes the shared `pnpm-lock.yaml` [#7672](https://github.com/pnpm/pnpm/issues/7672).
