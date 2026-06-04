---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fix `pnpm update --recursive --lockfile-only <pkg>@<version>` crashing with `Invalid Version` when the catalog entry for `<pkg>` is a version range (e.g. `^21.2.10`) and `catalogMode` is `strict` or `prefer`. The catalog–version comparison now skips the equality check when either side is a range rather than passing a range to `semver.eq()`, so range specifiers fall through to the existing mismatch handling instead of throwing [#11570](https://github.com/pnpm/pnpm/issues/11570).
