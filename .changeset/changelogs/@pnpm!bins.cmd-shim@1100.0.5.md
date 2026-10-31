## 1100.0.5

### Patch Changes

- On Windows, the shell shim of a `.cmd` or `.bat` bin now runs it under Git Bash when MSYS argument conversion is off, such as with `MSYS_NO_PATHCONV=1` or `MSYS2_ARG_CONV_EXCL='*'`. The shim used to start an interactive cmd.exe prompt instead [#17480](https://github.com/pnpm/pnpm/issues/17480).

- Commands installed with `pnpm add --global` now find the dependencies that pnpm hoists for their package. A package that loads a dependency it does not declare, such as a native addon's platform-specific package, failed with "Cannot find module". Reinstall such a package with `pnpm add --global` to update its existing command [#17503](https://github.com/pnpm/pnpm/issues/17503).
