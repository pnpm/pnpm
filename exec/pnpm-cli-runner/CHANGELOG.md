# @pnpm/exec.pnpm-cli-runner

## 1100.0.1

### Patch Changes

- 247d70b: Honor `--silent` when `verifyDepsBeforeRun: install` auto-installs dependencies before `pnpm run` or `pnpm exec`, preventing install output from being written to stdout [#11636](https://github.com/pnpm/pnpm/issues/11636).

## 1001.0.0

### Major Changes

- 491a84f: This package is now pure ESM.
- 7d2fd48: Node.js v18, 19, 20, and 21 support discontinued.

## 1000.0.1

### Patch Changes

- 0b0bcfa: Fix running pnpm CLI from pnpm CLI on Windows when the CLI is bundled to an executable [#8971](https://github.com/pnpm/pnpm/issues/8971).

## 1000.0.0

### Major Changes

- c52f55a: Initial release.
