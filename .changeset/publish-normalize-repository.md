---
"@pnpm/releasing.exportable-manifest": patch
"pnpm": patch
---

Normalize a string `repository` field into the `{ type, url }` object form when creating the publish manifest, matching npm's behavior. Some registries (e.g. Gitea/Codeberg) reject a string `repository` with a 500 Internal Server Error during `pnpm publish` [#12099](https://github.com/pnpm/pnpm/issues/12099).
