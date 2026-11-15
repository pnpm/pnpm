## 11.28.12

### Patch Changes

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build on macOS with the sandbox enabled. Installing again replaces the shims already in `node_modules` [#17750](https://github.com/pnpm/pnpm/issues/17750).

- Updated dependencies:
  - @pnpm/linux-arm64@11.28.12
  - @pnpm/linux-x64@11.28.12
  - @pnpm/linuxstatic-x64@11.28.12
  - @pnpm/macos-arm64@11.28.12
  - @pnpm/win-arm64@11.28.12
  - @pnpm/win-x64@11.28.12
