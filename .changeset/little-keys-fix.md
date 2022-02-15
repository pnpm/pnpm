---
"pnpm": minor
---

When using `pnpm <script>` as a shorthand for `pnpm run <script>`, unknown command line flags now throw an error. For example, `pnpm --unknown compile` now prints `Unknown option: 'unknown'` and exits the command. Previously these flags would not be passed to the underlying script, and pnpm would ignore the flag.
