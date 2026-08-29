## 1100.0.0

### Major Changes

- Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.

### Minor Changes

- Persist completed recursive tasks so `--resume-from` skips exactly the work that passed during a matching interrupted or failed `pnpm -r run` / `pnpm -r exec` invocation. When no compatible state exists, pnpm retains its graph-based resume behavior.

- Added per-task concurrency limits to workspace task orchestration. Set `tasks.<name>.concurrency` in `pnpm-workspace.yaml` to limit how many instances of that task may run across workspace projects at once:

  ```yaml
  tasks:
    build:
      concurrency: 2
  ```

### Patch Changes

- Published the workspace task graph and scheduler as `@pnpm/workspace.task-scheduler` so other workspace commands can use the same dependency-aware scheduling as recursive run and exec.

- Updated dependencies:
  - @pnpm/deps.graph-sequencer@1101.0.0
  - @pnpm/types@1102.1.0
