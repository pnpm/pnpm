---
"@pnpm/types": minor
"@pnpm/releasing.versioning": minor
"@pnpm/workspace.workspace-manifest-reader": minor
"pnpm": minor
"pacquet": minor
---

Added `versioning.epics` to `pnpm-workspace.yaml`. An epic ties a group of member packages to a lead package, constraining every member's major version to a band derived from the lead's major: while the lead is on major `M`, members live in `M*100 … M*100+99`. Members move independently inside the band (patch, minor, and a `major` intent that stays in-band), and when a release plan takes the lead to a new stable major, every member re-bases to the band floor in the same plan. Membership is matched with pnpm's package selectors — name globs, `./`-prefixed directory globs, and `!`-prefixed negations.
