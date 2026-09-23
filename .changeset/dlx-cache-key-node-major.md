---
"@pnpm/exec.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm dlx` now keeps a separate cache entry for each Node.js major version. A package built under one Node.js version, such as a native addon, is no longer reused under another [#8611](https://github.com/pnpm/pnpm/issues/8611).
