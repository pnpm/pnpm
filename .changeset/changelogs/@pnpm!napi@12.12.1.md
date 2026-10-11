## 12.12.1

### Patch Changes

- `rebuild()` with `skipIfHasSideEffectsCache` now skips a package whose cached build changed nothing inside the package, such as a native addon whose optional compilation failed. `install()` still runs the build scripts of such packages.
