---
"pacquet": patch
"pnpm": patch
"@pnpm/cli.default-reporter": patch
"@pnpm/config.package-is-installable": patch
"@pnpm/lockfile.filtering": patch
---

Fixed missing warnings when a required dependency does not support the current Node.js version.
