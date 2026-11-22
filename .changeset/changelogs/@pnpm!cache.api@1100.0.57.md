## 1100.0.57

### Patch Changes

- `pnpm cat-file` can read executable files from the store. `pnpm cache list`, `view`, and `delete` recognize mixed-case package names.

  `pnpm clean` removes custom virtual stores with hidden directory names. In Plug'n'Play projects it also removes the generated loader on a full clean.

- Updated dependencies:
  - @pnpm/config.reader@1102.4.0
  - @pnpm/resolving.npm-resolver@1104.2.12
  - @pnpm/store.cafs@1100.3.12
  - @pnpm/store.index@1101.0.5
