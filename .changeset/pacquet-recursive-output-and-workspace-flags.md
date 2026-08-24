---
"pacquet": minor
---

Added the six CLI flags the TypeScript pnpm CLI accepts but the Rust CLI did not [#14101](https://github.com/pnpm/pnpm/issues/14101):

- `--stream` prints a recursive command's script output as it arrives, one line at a time, prefixed with the project it came from, instead of letting the scripts write to the terminal directly. `--parallel` implies it, as in pnpm.
- `--aggregate-output` holds each script's streamed output until the script exits and then prints it as one block, so concurrent projects can't interleave.
- `--reporter-hide-prefix` drops that project prefix from the scripts' own output lines. On a recursive `pnpm exec`, the opposite spelling `--no-reporter-hide-prefix` turns the prefixing on.
- `--use-stderr` sends the reporter's output to stderr, leaving stdout for the command's own result.
- `--ignore-workspace` runs the command as if the project were standalone: no workspace root is discovered, so `pnpm-workspace.yaml` contributes neither settings nor sibling projects, and a blocked dependency build is not scaffolded into its `allowBuilds`.
- `--workspace-packages` overrides the `packages` patterns of `pnpm-workspace.yaml` for the run.

The `stream`, `aggregateOutput`, `reporterHidePrefix`, `useStderr`, and `ignoreWorkspace` settings are now read from `pnpm-workspace.yaml`, the global `config.yaml`, and their `PNPM_CONFIG_*` environment variables too.
