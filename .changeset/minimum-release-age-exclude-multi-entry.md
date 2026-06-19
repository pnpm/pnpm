---
"@pnpm/config.version-policy": patch
"pnpm": patch
---

Fixed `minimumReleaseAgeExclude` and `trustPolicyExclude` so multiple exact-version entries for the same package behave the same as a single `||` disjunction entry. Previously only the first matching rule's versions were honored, so a config like `[form-data@4.0.6, form-data@2.5.6]` could still flag `form-data@2.5.6` as violating `minimumReleaseAge`, while `[form-data@4.0.6 || 2.5.6]` worked as expected [#12463](https://github.com/pnpm/pnpm/issues/12463).
