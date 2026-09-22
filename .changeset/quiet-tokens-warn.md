---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

Warn when an empty environment variable removes an `.npmrc` authentication token. Authentication environment warnings now name the affected key [pnpm/pnpm#4806](https://github.com/pnpm/pnpm/issues/4806).
