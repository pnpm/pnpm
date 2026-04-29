---
"@pnpm/releasing.exportable-manifest": minor
"pnpm": minor
---

`pnpm pack` and `pnpm publish` now preserve the `packageManager` field and publish lifecycle scripts in generated package manifests, while continuing to omit the pnpm-specific `pnpm` field.
