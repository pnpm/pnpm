---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now prints config warnings, such as an unset environment variable in `.npmrc`, when loading the config fails.
