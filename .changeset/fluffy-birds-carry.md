---
"@pnpm/plugin-commands-patching": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/plugin-commands-audit": minor
"@pnpm/plugin-commands-rebuild": minor
"@pnpm/plugin-commands-store": minor
"@pnpm/dependency-path": minor
"@pnpm/lockfile-types": minor
"@pnpm/get-context": minor
"@pnpm/lockfile-file": minor
"@pnpm/core": minor
"@pnpm/config": minor
"pnpm": minor
---

**Semi-breaking.** Dependency key names in the lockfile are shortened if they are longer than 1000 characters. We don't expect this change to affect many users. This change is required to fix some edge cases in which installation fails with an out-of-memory error or "Invalid string length (RangeError: Invalid string length)" error. The max allowed length of the dependency key can be controlled with the `peers-suffix-max-length` setting [#8177](https://github.com/pnpm/pnpm/pull/8177).
