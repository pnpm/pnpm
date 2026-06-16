---
"@pnpm/deps.inspection.peers-checker": patch
"pnpm": patch
---

Fixed `pnpm peers check` to accept loose peer dependency ranges such as `>=3.16.0 || >=4.0.0-` when the installed peer version satisfies the range [#12149](https://github.com/pnpm/pnpm/issues/12149).
