---
"@pnpm/deps.inspection.outdated": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.resolver-base": patch
"pnpm": patch
"pacquet": patch
---

Fixed `pnpm outdated` to compare installed prereleases with the dist-tag that targets the same prerelease channel instead of only the stable `latest` tag [pnpm/pnpm#7339](https://github.com/pnpm/pnpm/issues/7339).
