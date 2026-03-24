---
"@pnpm/config.reader": minor
"pnpm": minor
---

Added a new setting `allowBuildsOfTrustedDeps` (default: `true`) that automatically loads a curated list of known-good packages from `@pnpm/plugin-trusted-deps` into `allowBuilds`. User-configured `allowBuilds` entries take precedence over the defaults. Set `allowBuildsOfTrustedDeps: false` to disable this behavior.
