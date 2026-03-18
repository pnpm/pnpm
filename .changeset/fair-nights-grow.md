---
"@pnpm/installing.resolve-dependencies": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/cli.default-reporter": patch
"@pnpm/deps.inspection.outdated": patch
---

Don't silently skip an optional dependency if it cannot be resolved from a version that satisfies the `minimumReleaseAge` setting [#10270](https://github.com/pnpm/pnpm/issues/10270).
