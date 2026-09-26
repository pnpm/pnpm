## 1100.0.44

### Patch Changes

- Bash completion now completes script names that contain a colon, such as `pnpm run test:u` to `pnpm run test:unit` [pnpm/pnpm#5482](https://github.com/pnpm/pnpm/issues/5482).

- Fish shell completion now omits names containing backslashes. The completion template could interpret them as escape sequences and inject extra completion records or terminal controls.

- Shell completion now omits candidates containing control or invisible formatting characters. Package and script names can no longer inject extra completion records or terminal escape sequences.

- Updated dependencies:
  - @pnpm/config.reader@1102.3.0
