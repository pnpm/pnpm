---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

The `pnpm:peer-dependency-issues` log event, which `--reporter ndjson` prints, no longer lists peers silenced by `peerDependencyRules.ignoreMissing` under `conflicts` or `intersections`. The event now carries the same issues as pnpm v12 and `pnpm peers check` [#8295](https://github.com/pnpm/pnpm/issues/8295).
