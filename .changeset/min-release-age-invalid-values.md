---
"@pnpm/config.version-policy": patch
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

`minimumReleaseAge` must now be a finite, non-negative number of minutes. Other values fail with `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE`. Before, such a value turned off the release age check or crashed the install.
