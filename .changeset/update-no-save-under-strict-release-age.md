---
"pacquet": patch
---

`pnpm update --no-save` no longer fails under `minimumReleaseAgeStrict` when every version it resolves is old enough. The command is refused only when a version younger than the cutoff is picked, because approving it would have to be written to `minimumReleaseAgeExclude` in `pnpm-workspace.yaml` [#14835](https://github.com/pnpm/pnpm/issues/14835).
