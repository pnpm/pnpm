---
"@pnpm/deps.path": patch
"@pnpm/config.version-policy": patch
"pnpm": patch
"pacquet": patch
---

Fixed `trustPolicyExclude` not exempting a package whose lockfile key carries a leading slash — the `/name@version` spelling lockfile format 6 used for registry packages [#13721](https://github.com/pnpm/pnpm/issues/13721).

`trustPolicyExclude` and `minimumReleaseAgeExclude` given a single string instead of a list are now read as a one-entry list. Such a value used to be read one character at a time, so the exclusion never matched — and a `*` anywhere in it exempted every package.
