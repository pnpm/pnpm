---
"@pnpm/installing.deps-installer": minor
"pnpm": minor
---

Adding [`pacquet`](https://github.com/pnpm/pnpm/tree/main/pacquet) (the Rust port of pnpm) to `configDependencies` in `pnpm-workspace.yaml` now delegates `pnpm install --frozen-lockfile` to the pacquet binary instead of running the JS installer's headless path. Pacquet emits the same `pnpm:*` NDJSON log events that `@pnpm/cli.default-reporter` already parses, so the install renders identically. Absent the `pacquet` entry, behavior is unchanged.

```yaml
# pnpm-workspace.yaml
configDependencies:
  pacquet: "^0.1.0"
```

This is an opt-in preview of the Rust install engine; it only activates on the frozen-install path (which pacquet already implements) and falls through to JS for any non-frozen install [#11723](https://github.com/pnpm/pnpm/issues/11723).
