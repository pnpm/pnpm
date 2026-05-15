---
"@pnpm/exec.lifecycle": patch
"pnpm": patch
---

Fix `ERR_PNPM_ENOENT` on `node_modules/.bin/<tool>` during `pnpm install` when an injected workspace package has a `prepare`/`postinstall` script and any dep with a bin entry. After lifecycle scripts ran, `runLifecycleHooksConcurrently` re-imported each workspace package into its injected targets via the store importer — a `scanDir` over the existing injected `node_modules` added absolute paths under it to the re-import's `filesMap`, then the importer's fast path (#11088) wiped the target directory before reading from those paths. Replace the round-trip with a direct file-by-file mirror that copies the post-script source tree into each `targetDir`. Source paths come from `fetchFromDir`, which excludes the source's `node_modules/`, so the target's existing `node_modules/` (bin links + transitive deps from the initial install) stays intact — no staging dir swap, no symlink dance.
