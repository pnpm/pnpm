---
"@pnpm/config.reader": patch
"pnpm": patch
---

`packageExtensions` is now validated when the configuration is read, so a malformed entry (for instance a dependency range set to `null`) fails with an actionable error instead of crashing later during peer dependency resolution [#13756](https://github.com/pnpm/pnpm/issues/13756).
