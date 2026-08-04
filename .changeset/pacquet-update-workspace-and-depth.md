---
"pacquet": minor
---

`pnpm update --workspace` is supported: dependencies that a workspace project publishes are re-pointed at the local copies through the `workspace:` protocol. The `saveWorkspaceProtocol` setting is honored — under its `rolling` default an entry becomes `workspace:*`, `workspace:^`, or `workspace:~` (whichever matches the range it already declared), so a sibling's next release does not invalidate it. Naming a dependency that is not in the workspace fails with `ERR_PNPM_WORKSPACE_PACKAGE_NOT_FOUND`, and combining the flag with `--latest` fails with `ERR_PNPM_BAD_OPTIONS`.

`pnpm update --depth <number>` is now applied per dependency instead of only distinguishing `0` from higher values: a dependency deeper than the given depth keeps its locked resolution, so `pnpm update --depth 0` updates direct dependencies only.
