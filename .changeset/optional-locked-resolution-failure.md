---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fail instead of silently removing an optional dependency's locked entries from `pnpm-lock.yaml` when the registry cannot resolve it. Previously, when registry metadata lacked a version that the lockfile already pinned (for example, a mirror that had not synced a recent release yet), `pnpm install` and `pnpm dedupe` silently dropped the optional dependency's entries — emptying maps such as the platform binaries of `@napi-rs/canvas` — so the lockfile differed between machines and frozen installs on other hosts had nothing to link [#12853](https://github.com/pnpm/pnpm/issues/12853).
