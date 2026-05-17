---
"@pnpm/resolving.npm-resolver": minor
"pnpm": patch
---

Sped up the `minimumReleaseAge` lockfile verification gate on cold-cache installs by trying npm's `/-/npm/v1/attestations/<name>@<version>` endpoint before fetching the full metadata document. The attestation response is tens of KB versus the multi-MB full metadata, so `--frozen-lockfile` installs against a fleet of provenance-published packages download far less to verify timestamps.

The publish time comes from `bundle.verificationMaterial.tlogEntries[].integratedTime` (the Rekor inclusion time, a couple of seconds after the actual publish — close enough for a policy that operates in minutes/hours/days). When the local full-metadata mirror already has the timestamp, or the attestation endpoint 404s / errors, the verifier falls back to the existing `fetchFullMetadataCached` path. Sigstore signature verification is not performed; the trust model is unchanged versus reading the registry's `time` field on the full metadata document [#11687](https://github.com/pnpm/pnpm/issues/11687).
