## 11.28.0

### Patch Changes

- Fallback `.cmd` and `.ps1` Windows wrappers in `@pnpm/exe` now propagate the exit status of the invoked `pnpm` command [pnpm/pnpm#14826](https://github.com/pnpm/pnpm/issues/14826).

- `pn`, `pnpx`, `pnx`, and `pnpm` now run when Git Bash, MSYS2, or Cygwin launches them through a Windows path such as `C:\Users\me\node_modules\pnpm\pn`. The aliases used to fail to find the pnpm installed beside them, or hand the call to an unrelated one [#14884](https://github.com/pnpm/pnpm/issues/14884).

- On Nix, a dependency's bin named like a system utility such as `sed` can no longer redirect a POSIX bin shim or the `pnpm`, `pn`, `pnpx`, and `pnx` launchers. The shims and launchers now ignore `node_modules` and relative `PATH` entries while they locate their own files. Installing again replaces the shims already in `node_modules` [#14883](https://github.com/pnpm/pnpm/issues/14883).

- Updated dependencies:
  - @pnpm/linux-arm64@11.28.0
  - @pnpm/linux-x64@11.28.0
  - @pnpm/linuxstatic-arm64@11.28.0
  - @pnpm/linuxstatic-x64@11.28.0
  - @pnpm/macos-arm64@11.28.0
  - @pnpm/win-arm64@11.28.0
  - @pnpm/win-x64@11.28.0
