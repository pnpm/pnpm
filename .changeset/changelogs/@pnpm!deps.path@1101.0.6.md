## 1101.0.6

### Patch Changes

- `pnpm list` now reports the correct package paths when `nodeLinker` is `hoisted` [#9593](https://github.com/pnpm/pnpm/issues/9593).

- Fixed two URL or local path dependencies sharing a virtual store directory when one URL had `+`, `#`, `:`, or `?` where the other had `/`. Such dependencies, including git dependencies pinned with `#`, now get a hash suffix on their directory name.

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.8
