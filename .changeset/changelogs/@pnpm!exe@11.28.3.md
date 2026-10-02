## 11.28.3

### Patch Changes

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build, where the system default path holds none of the utilities they call. Installing again replaces the shims already in `node_modules` [#16377](https://github.com/pnpm/pnpm/issues/16377).

- Updated dependencies:
  - @pnpm/linux-arm64@11.28.3
  - @pnpm/linux-x64@11.28.3
  - @pnpm/linuxstatic-x64@11.28.3
  - @pnpm/macos-arm64@11.28.3
  - @pnpm/win-arm64@11.28.3
  - @pnpm/win-x64@11.28.3
