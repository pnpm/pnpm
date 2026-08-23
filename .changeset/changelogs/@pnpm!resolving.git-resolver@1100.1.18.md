## 1100.1.18

### Patch Changes

- A git dependency installed over HTTPS from a hosted repository now keeps its branch, tag, or version range in the specifier recorded in `package.json`. It was written back without one, so the next `pnpm update` moved the dependency to the repository's default branch [#13999](https://github.com/pnpm/pnpm/issues/13999).

- Updated dependencies:
  - @pnpm/error@1100.1.3
  - @pnpm/network.fetch@1100.1.13
  - @pnpm/resolving.resolver-base@1101.1.1
