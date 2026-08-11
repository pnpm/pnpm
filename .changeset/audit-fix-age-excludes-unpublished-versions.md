---
"@pnpm/deps.compliance.commands": patch
"@pnpm/deps.compliance.audit": patch
"pnpm": patch
"pacquet": patch
---

`pnpm audit` no longer reports a patched version that was never published. The inferred patched range (e.g. `>=2.0.3` from `<=2.0.2`) is now checked against the registry packument, and when no published version satisfies it the report shows `Patched versions: None`. This also prevents `pnpm audit --fix` from adding overrides or `minimumReleaseAgeExclude` entries for patches that do not exist [#13824](https://github.com/pnpm/pnpm/issues/13824).

`pnpm audit --fix` and `pnpm audit --fix update` no longer add a `minimumReleaseAgeExclude` entry when the registry packument shows that the minimum patched version was never published. Previously such entries were written for versions that do not exist, which would have let a later publish of that version bypass the `minimumReleaseAge` gate [#11563](https://github.com/pnpm/pnpm/issues/11563).
