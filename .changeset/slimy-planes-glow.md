---
"@pnpm/config": minor
"pnpm": minor
---

A new setting, `exclude-links-from-lockfile`, is now supported. When enabled, specifiers of local linked dependencies won't be duplicated in the lockfile.

This setting was primarily added for use by [Bit CLI](https://github.com/teambit/bit), which links core aspects to `node_modules` from external directories. As such, the locations may vary across different machines, resulting in the generation of lockfiles with differing locations.
