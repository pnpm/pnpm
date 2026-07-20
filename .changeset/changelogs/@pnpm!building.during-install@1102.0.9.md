## 1102.0.9

### Patch Changes

- When a dependency's build script fails under `enableGlobalVirtualStore`, the global virtual store directory it was being built in is now removed for scoped packages too. Previously the cleanup resolved one directory level short of the hash directory for a scoped name, leaving a half-built directory behind that later installs would reuse.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.20
  - @pnpm/config.reader@1101.12.3
  - @pnpm/exec.lifecycle@1100.1.6
