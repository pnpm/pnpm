---
"pacquet": patch
---

Fixed resolution against registries whose packuments differ from npm's in ways pnpm did not model. A version manifest pnpm cannot read is skipped as though the registry never published that version, so a single unexpected field shape could remove a package's newest releases — including whichever one `latest` pointed at — from resolution, surfacing as a misleading "no version found for the latest tag".

Manifest fields pnpm reads but never installs from — the publisher and attestation records, `dist.unpackedSize`, `dist.fileCount`, and `peerDependenciesMeta.<name>.optional` — now degrade on their own instead of taking the version with them. `dist.integrity` still fails the version, since a tarball hash pnpm cannot read is one it cannot verify.

When `latest` names a version whose manifest cannot be read, the error now says so and names the field pnpm choked on, instead of reporting the tag as empty.
