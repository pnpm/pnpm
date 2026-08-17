---
"@pnpm/config.version-policy": patch
"pnpm": patch
---

Fixed `trustPolicyExclude` and `minimumReleaseAgeExclude` being ignored when set to a single string instead of a list. The value was read one character at a time, so the exclusion never matched the package it named — and a `*` anywhere in it matched every package, silently switching the policy off.
