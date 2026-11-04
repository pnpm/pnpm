---
"pacquet": minor
---

Rerunning `pnpm publish` to a registry declared `serverType: pnpr` after the version was already published now succeeds when the published version is the same release. pnpm compares the version's release record (the archive that targets `any` and each platform variant), its provenance statement when the rerun signs one, and the dist-tag. When the published version differs, the publish fails with `ERR_PNPM_PUBLISHED_RELEASE_DIFFERS`, which names what differs.
