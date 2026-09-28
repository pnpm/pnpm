## 11.28.1

### Patch Changes

- `pnpm setup` no longer writes the `pn.ps1`, `pnpx.ps1`, and `pnx.ps1` PowerShell wrappers. It also removes the ones an earlier setup wrote. PowerShell now runs `pn`, `pnpx`, and `pnx` through their `.cmd` wrappers, like `pnpm` itself. Before, these aliases failed with a "not digitally signed" error wherever the execution policy blocks unsigned scripts [#8444](https://github.com/pnpm/pnpm/issues/8444).

- `@pnpm/exe` no longer ships a binary for arm64 musl Linux, such as Alpine on ARM. The published binary crashed with a segmentation fault at startup. Installing `@pnpm/exe` on that platform now fails with an error that suggests `npm install -g pnpm` or pnpm 12 instead [#10443](https://github.com/pnpm/pnpm/issues/10443).

- On Windows, installing `@pnpm/exe` with npm inside a project now writes `node_modules/.bin` shims that run the standalone executable [#15688](https://github.com/pnpm/pnpm/issues/15688).

- On Windows, globally installed `@pnpm/exe` commands now run in the invoking PowerShell console and return their exit status [pnpm/pnpm#6503](https://github.com/pnpm/pnpm/issues/6503).

- Updated dependencies:
  - @pnpm/linux-arm64@11.28.1
  - @pnpm/linux-x64@11.28.1
  - @pnpm/linuxstatic-x64@11.28.1
  - @pnpm/macos-arm64@11.28.1
  - @pnpm/win-arm64@11.28.1
  - @pnpm/win-x64@11.28.1
