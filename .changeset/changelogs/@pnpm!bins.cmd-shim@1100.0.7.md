## 1100.0.7

### Patch Changes

- Linking bins is faster for packages that ship large native executables as bins. pnpm now reads only the first 512 bytes of a bin file to find its shebang [#17361](https://github.com/pnpm/pnpm/issues/17361).

- `pnpm run` and `pnpm setup` no longer clear the visible terminal on Windows when a project path contains non-ASCII characters. Command shims and `pnpm setup` now switch the console code page with `mode.com con cp select=` instead of `chcp.com`; changing the code page with `chcp.com` clears the screen.

- POSIX bin shims and the `pnpm`, `pn`, `pnpx`, and `pnx` launchers now run inside a Nix build on macOS with the sandbox enabled. Installing again replaces the shims already in `node_modules` [#17750](https://github.com/pnpm/pnpm/issues/17750).

- Bin shims in the global virtual store no longer include the hoisted modules directory of the project that installed them [#18035](https://github.com/pnpm/pnpm/issues/18035).

- Updated dependencies:
  - @pnpm/fs.graceful-fs@1100.2.6
