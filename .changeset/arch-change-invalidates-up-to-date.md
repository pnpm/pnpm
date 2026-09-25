---
"@pnpm/workspace.state": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now detects a `supportedArchitectures` change and re-evaluates previously skipped platform-specific optional dependencies, instead of reporting the project as up to date and leaving the packages for the old architecture set in place.
