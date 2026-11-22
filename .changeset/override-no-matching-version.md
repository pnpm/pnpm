---
"@pnpm/installing.deps-resolver": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/cli.default-reporter": patch
"pnpm": patch
---

When an overrides entry requests a version that does not exist on the registry, `pnpm install` now names that override entry in the error. If the entry's selector has a version range, such as `foo@^1`, the error also shows the latest published version in that range.
