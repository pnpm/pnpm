## 1100.4.9

### Patch Changes

- `syncInjectedDepsAfterScripts` now removes the bin link of a bin the script dropped. Previously only new bins were linked, so a build step that stopped declaring one left its shim behind, pointing at a command that was no longer there.

- Updated dependencies:
  - @pnpm/building.commands@1100.1.22
  - @pnpm/cli.utils@1101.0.23
  - @pnpm/config.reader@1101.17.0
  - @pnpm/config.version-policy@1100.2.0
  - @pnpm/deps.status@1100.1.17
  - @pnpm/engine.runtime.commands@1100.1.21
  - @pnpm/error@1100.1.2
  - @pnpm/exec.lifecycle@1100.1.13
  - @pnpm/installing.client@1100.3.4
  - @pnpm/installing.commands@1100.15.0
  - @pnpm/pkg-manifest.reader@1100.0.16
  - @pnpm/store.path@1100.0.5
  - @pnpm/workspace.injected-deps-syncer@1100.0.33
  - @pnpm/workspace.project-manifest-reader@1100.0.24
