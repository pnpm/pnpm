## 12.3.3

### Patch Changes

- Fixed concurrent installs sharing a store occasionally failing with an ENOENT error while importing a package file [#14353](https://github.com/pnpm/pnpm/issues/14353).

- Sped up writing the lockfile in large workspaces [#14352](https://github.com/pnpm/pnpm/issues/14352).

- Sped up dependency resolution in large workspaces [#14352](https://github.com/pnpm/pnpm/issues/14352).

- pnpm now runs through Node.js when it was installed by a tool that skips build scripts, such as Vercel's `packageManager` provisioning, Bun, Deno, or `npm install --ignore-scripts`. Those installs previously failed with `syntax error near unexpected token ')'`. They still cannot run pnpm on Windows. On macOS only a shell can start it [#14346](https://github.com/pnpm/pnpm/issues/14346).
