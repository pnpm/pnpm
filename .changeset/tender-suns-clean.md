---
"@pnpm/tarball-resolver": patch
---

When installing dependencies via URL redirection and writing them to `package.json`, remove the trailing slash from the URL to maintain consistency with npm.
