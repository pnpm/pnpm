---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

Make `pnpm self-update` respect `minimumReleaseAge` (and `minimumReleaseAgeExclude`) when resolving which pnpm version to install.

When the `latest` dist-tag points to a version newer than the configured age threshold, `self-update` now selects the newest mature version instead unless excluded by `minimumReleaseAgeExclude`.
