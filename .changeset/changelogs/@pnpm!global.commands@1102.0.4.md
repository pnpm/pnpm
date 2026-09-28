## 1102.0.4

### Patch Changes

- `pnpm env remove --global` deletes Node.js versions that pnpm installed into its own store, including when another tool installed pnpm [pnpm/pnpm#8357](https://github.com/pnpm/pnpm/issues/8357).

- `pnpm update --global` now reinstalls the global packages that pnpm 10 installed into the previous global directory, `<global-dir>/5`, so their commands are linked into the pnpm home `bin` directory again and `pnpm list --global` lists them. Once every package is migrated, pnpm deletes the previous directory and the commands pnpm 10 linked into the pnpm home [#11528](https://github.com/pnpm/pnpm/issues/11528).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.34
  - @pnpm/bins.remover@1100.0.26
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/deps.inspection.list@1101.0.7
  - @pnpm/error@1100.2.1
  - @pnpm/global.packages@1101.1.5
  - @pnpm/installing.deps-installer@1104.2.1
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/pkg-manifest.utils@1100.4.7
  - @pnpm/resolving.local-resolver@1101.2.4
  - @pnpm/resolving.parse-wanted-dependency@1100.0.3
  - @pnpm/store.connection-manager@1101.3.0
