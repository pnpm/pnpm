## 1100.0.4

### Patch Changes

- `pnpm publish` now allows a detached Git HEAD in CI, including checkouts of release tags. The working tree must still be clean. Branch and remote-history checks still apply when HEAD is attached [pnpm/pnpm#5894](https://github.com/pnpm/pnpm/issues/5894).
