---
"@pnpm/releasing.versioning": minor
"@pnpm/releasing.commands": minor
"pnpm": minor
"pacquet": minor
---

The first release of a package now publishes the version written in its manifest verbatim, instead of bumping off it. `pnpm version -r` and `pnpm change status` check the registry for each release's current version; when that version is not yet published, the package debuts at it and its pending changesets apply only from the next release. A newly added package seeded at `1100.0.0` with a `minor` changeset is therefore published as `1100.0.0` rather than skipping straight to `1100.1.0`.
