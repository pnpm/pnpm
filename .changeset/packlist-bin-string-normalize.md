---
"@pnpm/fs.packlist": patch
"pnpm": patch
---

`@pnpm/fs.packlist` now normalizes a string `bin` field in an unnormalized manifest, so the binary file is always included even when excluded by `files` or `.npmignore`.
