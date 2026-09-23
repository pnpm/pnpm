---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

The `pnpm:peer-dependency-issues` log event, which `--reporter ndjson` prints, no longer lists peers silenced by `peerDependencyRules.ignoreMissing` under `conflicts` or `intersections` [#8295](https://github.com/pnpm/pnpm/issues/8295).
