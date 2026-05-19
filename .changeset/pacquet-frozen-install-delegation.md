---
"@pnpm/installing.commands": minor
"@pnpm/installing.deps-installer": minor
"pnpm": minor
---

Adding [`pacquet`](https://github.com/pnpm/pnpm/tree/main/pacquet) (the Rust port of pnpm) to `configDependencies` in `pnpm-workspace.yaml` now delegates the materialization phase of `pnpm install` to the pacquet binary instead of running the JS installer's headless path. Pacquet emits the same `pnpm:*` NDJSON log events that `@pnpm/cli.default-reporter` already parses, so the install renders identically. Absent the `pacquet` entry, behavior is unchanged.

```yaml
# pnpm-workspace.yaml
configDependencies:
  pacquet: "^0.1.0"
```

Pacquet takes over every place pnpm would otherwise call `headlessInstall`: the frozen-install path, the hoisted-`nodeLinker` install, the workspace partial-install (where pnpm runs a `lockfileOnly` resolve pass first), and the agent-server install. In all cases pnpm still owns dependency resolution; pacquet only fetches and imports from the freshly-written lockfile. This is an opt-in preview of the Rust install engine [#11723](https://github.com/pnpm/pnpm/issues/11723).
