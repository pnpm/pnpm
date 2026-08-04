---
"pacquet": patch
---

`pnpm add <pkg>` (without a version) and `pnpm update --latest` now resolve the `latest` dist-tag through the `minimumReleaseAge`-aware picker, pinning the newest version that satisfies the cutoff instead of writing a range the follow-up install rejects. An invalid `minimumReleaseAgeExclude` value reported by these commands now carries the same `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE` error code the install reports. See pnpm/pnpm#11165.
