## 1101.0.4

### Patch Changes

- pnpm now escapes trailing dots and spaces in `node_modules/.pnpm` directory names. Windows strips these characters, so a dependency such as `"parent-pkg": "file:../"` created a directory that could not be deleted or failed to install [#8101](https://github.com/pnpm/pnpm/issues/8101).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/types@1102.1.1
