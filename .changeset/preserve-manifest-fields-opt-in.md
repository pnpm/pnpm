---
"@pnpm/releasing.commands": minor
"@pnpm/releasing.exportable-manifest": minor
"@pnpm/config.reader": minor
"pnpm": minor
---

Add a `preserve-manifest-fields` option for `pnpm pack` and `pnpm publish`. When enabled, the original `packageManager` field and publish lifecycle scripts are preserved in the packed/published manifest instead of being stripped. The pnpm-specific `pnpm` field continues to be omitted.
