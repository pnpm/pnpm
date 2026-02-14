---
"@pnpm/outdated": patch
"pnpm": patch
---

Fixed `pnpm outdated` crashing with `ERR_PNPM_NO_MATCHING_VERSION` when `minimumReleaseAge` is set and all versions of a package are newer than the threshold [#10605](https://github.com/pnpm/pnpm/issues/10605).
