---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fixed `pnpm publish --provenance` failing with a 422 from the registry when the package version contained semver build metadata (e.g. `1.0.0-canary.0+abc1234`). The `+<build>` segment is now stripped before packing so that the version embedded in the tarball, the metadata sent to the registry, and the sigstore provenance subject all agree [#11518](https://github.com/pnpm/pnpm/issues/11518).
