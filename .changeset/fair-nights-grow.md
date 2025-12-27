---
"@pnpm/resolve-dependencies": patch
"@pnpm/npm-resolver": patch
"@pnpm/default-reporter": patch
"@pnpm/outdated": patch
---

Don't silently skip an optional dependency if it cannot be resolved from a version that satisfies the `minimumReleaseAge` setting [#10270](https://github.com/pnpm/pnpm/issues/10270).
