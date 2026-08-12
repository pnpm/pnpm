---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Projects using `resolutionMode: time-based` now benefit from the fast lockfile update paths. A removal, a dependency group move, or a compatible range change no longer forces a full re-resolution just because the lockfile carries a `time` field [#13696](https://github.com/pnpm/pnpm/issues/13696).
