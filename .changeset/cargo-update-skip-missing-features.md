---
"pacquet": patch
---

`pnpm update` now keeps a Cargo dependency at its current version, with a warning, if the new version lacks a feature the workspace enables on it. Cargo could not resolve such a version, so the whole update failed. If the crate is named on the command line, as in `pnpm update --latest crate:sha2`, the update fails with an error that names the missing features.
