## 1100.1.1

### Patch Changes

- Scripts that `pnpx` and `pnx` run now get pnpm itself as `npm_execpath`. A script that ran `$npm_execpath install` there ran `pnpm dlx install`.

- Updated dependencies:
  - @pnpm/types@1102.1.1
