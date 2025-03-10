# @pnpm/crypto.polyfill

## 1000.1.0

### Minor Changes

- 58d8597: Fix the type of `hash`. It was `any` because `crypto.hash` not being declared would fall back to `any`.

## 1.0.0

### Major Changes

- 222d10a: Initial release.

### Patch Changes

- 222d10a: Use `crypto.hash`, when available, for improved performance [#8629](https://github.com/pnpm/pnpm/pull/8629).
