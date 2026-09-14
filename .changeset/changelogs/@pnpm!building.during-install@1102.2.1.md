## 1102.2.1

### Patch Changes

- A patch that gives a dependency a `preinstall`, `install`, or `postinstall` script, or a `binding.gyp`, now runs that build. pnpm asks for build approval first, so the package is listed under "Ignored build scripts" until it is allowed to build. pnpm 12 ran nothing, and pnpm 11 ran it without asking [#14648](https://github.com/pnpm/pnpm/issues/14648).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.31
  - @pnpm/building.pkg-requires-build@1100.0.17
  - @pnpm/config.reader@1102.2.0
  - @pnpm/deps.graph-hasher@1100.3.2
  - @pnpm/deps.path@1101.0.2
  - @pnpm/exec.lifecycle@1100.1.17
  - @pnpm/fs.hard-link-dir@1100.0.5
  - @pnpm/pnpr.client@3.0.2
