---
"pacquet": patch
---

`pnpm update --no-save` no longer fails under `minimumReleaseAgeStrict` when every version it resolves is old enough. The command is refused only when it picks a version younger than the cutoff [#14835](https://github.com/pnpm/pnpm/issues/14835).
