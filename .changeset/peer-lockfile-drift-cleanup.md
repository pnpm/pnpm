---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm install` no longer leaves peer dependency suffixes on unrelated packages when regenerating the lockfile after a manifest edit. The locked-peer-context reuse pass still resolves peers as before, but a following cleanup drops, per package, the peer suffixes a fresh resolution (and `pnpm dedupe`) would not have produced, so the deduplicated instances collapse during a plain `pnpm install`. This fixes lockfile drift where an unrelated `pnpm install` would add peer suffixes that `pnpm dedupe` would immediately remove.
