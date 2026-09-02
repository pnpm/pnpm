---
"pacquet": patch
---

Fixed resolution against registries whose packuments differ from npm's in ways pnpm did not model. A version manifest pnpm cannot read is skipped as though the registry never published that version, so a single unexpected field shape could remove a package's newest releases — including whichever one `latest` pointed at — from resolution, surfacing as a misleading "no version found for the latest tag".

Fields pnpm reads but does not depend on now degrade on their own instead of taking the version with them:

- `dist.attestations.provenance`, `_npmUser.trustedPublisher`, and `_npmUser.approver` are treated as the presence signals they are, keeping the published details when the registry sends an object and accepting any other shape.
- `dist.unpackedSize` and `dist.fileCount` accept any integral encoding, including the floats and numeric strings a registry's serializer may produce.
- `_npmUser` accepts a non-object in place of the publisher record.
- `peerDependenciesMeta.<name>.optional` accepts a non-boolean, which counts as unset — matching how the flag is compared.

`dist.integrity` stays strict: a version whose tarball hash pnpm cannot read is one it cannot verify, and that has to fail rather than resolve.

When `latest` does resolve to a version whose manifest cannot be read, the error now names that version and the field pnpm choked on instead of reporting the tag as empty.
