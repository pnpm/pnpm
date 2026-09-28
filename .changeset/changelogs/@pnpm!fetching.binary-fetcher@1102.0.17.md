## 1102.0.17

### Patch Changes

- Installing a runtime from a zip archive, such as Node.js on Windows, Deno, or Bun, uses less memory [#14164](https://github.com/pnpm/pnpm/issues/14164).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/error@1100.2.1
  - @pnpm/fetching.fetcher-base@1100.2.12
  - @pnpm/store.index@1100.3.3
