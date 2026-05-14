---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Fix `minimumReleaseAge` handling for cached abbreviated metadata.

The version-spec cache fast path no longer rethrows `ERR_PNPM_MISSING_TIME` under `strictPublishedByCheck`; it now falls through to the registry-fetch path, consistent with the adjacent mtime-gated cache block.

When the registry returns 304 Not Modified for a package whose cached metadata is abbreviated (no per-version `time`), pnpm now re-fetches with `fullMetadata: true` if `minimumReleaseAge` is active and the package was modified after the cutoff. The upgraded metadata is persisted to disk so subsequent installs don't repeat the fetch. Previously the abbreviated meta was used as-is and the maturity check fell back to its warn-and-skip path, silently bypassing the quarantine and emitting a misleading "metadata is missing the time field" warning.

Closes #11619.
