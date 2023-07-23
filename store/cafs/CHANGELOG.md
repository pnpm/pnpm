# @pnpm/store.cafs

## 1.0.1

### Patch Changes

- fe1c5f48d: The length of the temporary file names in the content-addressable store reduced in order to prevent `ENAMETOOLONG` errors from happening [#6842](https://github.com/pnpm/pnpm/issues/6842).

## 1.0.0

### Major Changes

- 4bbf482d1: The package is renamed from `@pnpm/cafs` to `@pnpm/store.cafs`.

  The content-addressable store locker should be only created once per process. This fixes an issue that started happening after merging [#6817](https://github.com/pnpm/pnpm/pull/6817)
