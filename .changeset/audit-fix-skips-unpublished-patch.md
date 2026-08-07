---
"@pnpm/deps.compliance.commands": patch
"pnpm": patch
---

`pnpm audit --fix=override` no longer writes an override pointing at a version that was never published. Advisories with a `<=X.Y.Z` range have their patched version guessed as `X.Y.Z+1`, which doesn't exist when the fix only landed in a later major (or never landed at all); the resulting override made every following install fail with `ERR_PNPM_NO_MATCHING_VERSION`. Such advisories are now left unfixed instead [#12651](https://github.com/pnpm/pnpm/issues/12651).
