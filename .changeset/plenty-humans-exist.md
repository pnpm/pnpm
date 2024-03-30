---
"@pnpm/config": major
"pnpm": major
---

Revert a change in v9.0.0-alpha.0 to use the same directories on macOS as on Linux. [#7321](https://github.com/pnpm/pnpm/issues/7321). The directories inside `~/Library` will be used again on macOS. [#7732](https://github.com/pnpm/pnpm/issues/7732)
