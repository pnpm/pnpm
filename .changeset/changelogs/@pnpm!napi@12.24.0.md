## 12.24.0

### Patch Changes

- Fixed the FreeBSD x64 build of `pnpm`, which failed to compile.

- The `@pnpm/napi` addon is 9 to 12 MB smaller on Linux and macOS. It no longer includes the symbol names that only debuggers use.
