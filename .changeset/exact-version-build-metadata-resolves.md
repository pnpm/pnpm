---
"pacquet": patch
---

A dependency pinned to an exact version carrying semver build metadata (`"@parcel/codeframe": "2.0.0-canary.1718+d8408010f"`) installs again instead of failing with `ERR_PNPM_NO_MATCHING_VERSION` [#14096](https://github.com/pnpm/pnpm/issues/14096). npm drops build metadata when it publishes a version, so the metadata is dropped from the version that is looked up too, as npm and pnpm v11 do.
