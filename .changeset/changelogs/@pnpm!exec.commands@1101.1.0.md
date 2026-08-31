## 1101.1.0

### Minor Changes

- Persist completed recursive tasks so `--resume-from` skips exactly the work that passed during a matching interrupted or failed `pnpm -r run` / `pnpm -r exec` invocation. When no compatible state exists, pnpm retains its graph-based resume behavior.

- Added per-task concurrency limits to workspace task orchestration. Set `tasks.<name>.concurrency` in `pnpm-workspace.yaml` to limit how many instances of that task may run across workspace projects at once:

  ```yaml
  tasks:
    build:
      concurrency: 2
  ```

- Workspace task orchestration ([pnpm/rfcs#23](https://github.com/pnpm/rfcs/pull/23)). `pnpm -r run` and `pnpm -r exec` now schedule per task instead of in topological chunks: a task starts as soon as the tasks it depends on have finished, so a project no longer waits for unrelated projects that happen to share its chunk.

  A new `tasks` section in `pnpm-workspace.yaml` declares what a task depends on, using the `^` convention:

  ```yaml
  tasks:
    build:
      dependsOn: ['^build']
    test:
      dependsOn: ['build']
    lint: {}
  ```

  `^name` means the named task in each of the project's workspace dependencies; a bare `name` means the task in the same project; an entry with no `dependsOn` declares an empty dependency list. A task with no entry behaves as `dependsOn: ['^<its own name>']`, which is exactly what the previous chunked ordering implied — an unconfigured workspace gets the scheduler improvement and nothing else changes meaning. A project without the script is reported skipped and passes its edges through to its own dependencies, so a scriptless package does not sever a chain.

  Also part of this change:

  - A dependency cycle among the tasks of a run is now an error naming the participating tasks (`ERR_PNPM_TASK_CYCLE`) instead of silently running in an arbitrary order. Setting `ignoreWorkspaceCycles: true` downgrades the error to a warning: the cycle's tasks run in an arbitrary order relative to each other.
  - `--resume-from` now skips exactly the transitive dependencies of the anchor package; work unrelated to the anchor still runs.
  - Under `--no-bail`, tasks whose dependencies failed are reported as skipped, not failed, and do not add to the exit code.
  - With `--bail` (the default), the first failure still ends the run at once and nothing new is dispatched — including scripts already queued behind the concurrency limit.
  - `pnpm -r run --dry-run <script>` prints the task graph that would execute without running anything (including skipping the `verifyDepsBeforeRun` check); `--json` emits the tasks and their resolved dependency edges.
  - Output is inherited rather than piped only when at most one script can ever be in flight (`--workspace-concurrency=1`, or the graph forces the scripts to run one after another).

### Patch Changes

- Treat empty scripts selected by a regular expression as missing before running dependent tasks.

- Filter hidden scripts matched by a regular expression during recursive runs when a visible script also matches.

- `--production` is accepted again as an alias of `--prod` on `install`, `fetch`, `prune`, `update`, `list`, `why`, and `sbom`, and the install that `verifyDepsBeforeRun` reproduces is now spelled with `--prod`. `pnpm run` no longer aborts with "unexpected argument '--production' found" after a production-only install [#14147](https://github.com/pnpm/pnpm/issues/14147).

- Published the workspace task graph and scheduler as `@pnpm/workspace.task-scheduler` so other workspace commands can use the same dependency-aware scheduling as recursive run and exec.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.16
  - @pnpm/building.commands@1101.2.0
  - @pnpm/cli.utils@1101.0.25
  - @pnpm/config.reader@1102.1.0
  - @pnpm/config.version-policy@1100.2.2
  - @pnpm/core-loggers@1100.3.4
  - @pnpm/crypto.hash@1100.0.3
  - @pnpm/deps.status@1100.1.20
  - @pnpm/engine.runtime.commands@1101.1.0
  - @pnpm/exec.lifecycle@1100.1.15
  - @pnpm/installing.client@1100.3.7
  - @pnpm/installing.commands@1101.1.0
  - @pnpm/pkg-manifest.reader@1100.0.18
  - @pnpm/types@1102.1.0
  - @pnpm/workspace.injected-deps-syncer@1100.0.35
  - @pnpm/workspace.project-manifest-reader@1100.0.26
  - @pnpm/workspace.projects-sorter@1101.0.0
