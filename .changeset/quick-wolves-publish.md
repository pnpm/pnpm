---
"@pnpm/releasing.commands": patch
"pnpm": patch
"pacquet": minor
---

Batch workspace publishing accepts a shared scope-specific credential, rejects mismatched credentials for a registry before publishing, and runs the `publish` and `postpublish` scripts after each completed registry group [pnpm/pnpm#14101](https://github.com/pnpm/pnpm/issues/14101).
