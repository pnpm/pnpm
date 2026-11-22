---
"pacquet": patch
---

`pnpm pipeline` invalidates cached results when values supplied by `--env-file`, `extraEnv`, or `NODE_OPTIONS` change, or when the script shell settings change. Declared outputs of sibling tasks no longer invalidate the default inputs, and output globs support `!` exclusions.

`pnpm pipeline` respects `--filter` and `--filter-prod`, includes untracked files when selecting affected projects, and runs the prerequisites of tasks matched by regex selectors.

With `shellEmulator: true`, parameter expansions such as `${MODE:-dev}` now see assignments made earlier in the script.
