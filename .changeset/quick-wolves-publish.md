---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": minor
---

Added batch workspace publishing to the Rust CLI. Batch publishing accepts a common scope-specific credential, rejects mismatched credentials before publishing, and runs post-publish scripts after each completed registry group [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).
