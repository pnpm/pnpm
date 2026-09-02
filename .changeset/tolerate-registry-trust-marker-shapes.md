---
"pacquet": patch
---

Fixed resolution against registries whose version manifests carry `_npmUser`, `dist.attestations`, `dist.unpackedSize`, `dist.fileCount`, or `peerDependenciesMeta` in a shape npm does not use. Such a version was skipped as though it had never been published, so `pnpm add` could fail with "no version found for the latest tag" even though the registry served it.
