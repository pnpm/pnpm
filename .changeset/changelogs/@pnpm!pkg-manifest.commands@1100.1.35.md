## 1100.1.35

### Patch Changes

- `pnpm pkg fix` now converts a `repository` string such as `github:pnpm/example` to the object form, as `npm pkg fix` does. It also normalizes a GitHub, GitLab, or Bitbucket repository URL the way `npm pkg fix` does [pnpm/pnpm#16980](https://github.com/pnpm/pnpm/issues/16980).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.33
  - @pnpm/config.reader@1102.3.6
  - @pnpm/types@1102.1.2
