---
"@pnpm/config": patch
"pnpm": patch
---

When the global bin directory is set to a symlink, check not only the symlink in the PATH but also the target of the symlink [#4744](https://github.com/pnpm/pnpm/issues/4744).
