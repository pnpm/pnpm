---
"pacquet": patch
---

`pnpm dedupe` no longer double-counts reused packages in warm full runs. The resolution observer and the fetch/materialization phases each emitted `found_in_store` for the same packages, so the progress summary could report more reused packages than resolved ones (e.g. `resolved 69, reused 138`). The observer now skips the resolve-time store reporting for full runs — those phases already report each store hit exactly once — while `--lockfile-only` and `--check` runs keep it, since they run no fetch or materialization [#15303](https://github.com/pnpm/pnpm/issues/15303).
