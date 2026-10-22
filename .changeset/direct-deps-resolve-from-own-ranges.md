---
"pacquet": patch
---

When `autoDedupe` is off, a workspace project's dependencies now resolve from its own ranges, like they do with `sharedWorkspaceLockfile: false`. Another project's narrower range no longer holds them back. For example, `ajv: "*"` now resolves to the latest Ajv even if another project depends on `ajv@^6`. Packages that a project lists in `peerDependencies` still follow the versions other projects use. `pnpm dedupe` and installs with `autoDedupe: true` still pick one version that satisfies every range. Set `autoDedupe: true` if CI runs `pnpm dedupe --check` [#17235](https://github.com/pnpm/pnpm/issues/17235).
