---
"@pnpm/config.reader": minor
"pnpm": minor
---

Added a new setting `allow-builds-for-trusted-deps` (default: `true`) that automatically loads a curated list of known-good packages from `@pnpm/plugin-trusted-deps` into `allowBuilds`. User-configured `allowBuilds` entries take precedence over the defaults. Set `allow-builds-for-trusted-deps=false` to disable this behavior.
