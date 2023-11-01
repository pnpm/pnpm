---
"@pnpm/build-modules": patch
"pnpm": patch
---

Fixed a regression that was shipped with pnpm v8.10.0. Dependencies that were already built should not be rebuilt on repeat install. This issue was introduced via the changes related to [supportedArchitectures](https://github.com/pnpm/pnpm/pull/7214). Related issue [#7268](https://github.com/pnpm/pnpm/issues/7268).
