---
"pnpm": patch
---

`pnpm outdated` now reports newer prereleases of an installed prerelease. It checks the dist-tag that points to the newest version of the installed prerelease channel, such as `next` for `3.0.0-rc.0`, alongside `latest` [#7339](https://github.com/pnpm/pnpm/issues/7339).
