---
"@pnpm/resolve-dependencies": patch
"pnpm": patch
---

Fix aliased dependencies resolution on repeat install with existing lockfile, when the aliased dependency doesn't specify a version or range [#7957](https://github.com/pnpm/pnpm/issues/7957).
