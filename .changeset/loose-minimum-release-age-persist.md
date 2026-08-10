---
"pacquet": patch
---

`pnpm install` again records immature versions picked under `minimumReleaseAge` (when `minimumReleaseAgeStrict` is off) in `minimumReleaseAgeExclude` in `pnpm-workspace.yaml`, so a later frozen install of the same lockfile passes verification [#13687](https://github.com/pnpm/pnpm/issues/13687).
