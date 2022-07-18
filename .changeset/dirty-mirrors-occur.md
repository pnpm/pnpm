---
"@pnpm/build-modules": minor
"@pnpm/config": minor
"@pnpm/core": minor
"@pnpm/headless": minor
"@pnpm/hoist": minor
"@pnpm/link-bins": minor
"pnpm": minor
---

A new setting supported: `prefer-symlinked-executables`. When `true`, pnpm will create symlinks to executables in
`node_modules/.bin` instead of command shims (but on POSIX systems only).

This setting is `true` by default when `node-linker` is set to `hoisted`.

Related issue: [#4782](https://github.com/pnpm/pnpm/issues/4782).
