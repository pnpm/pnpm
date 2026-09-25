---
"pacquet": patch
---

When pnpm switches to the version a project pins, the `minimumReleaseAge` approvals for that version are now added to `minimumReleaseAgeExclude` in the project's `pnpm-workspace.yaml`. A project without that file gets one. Global commands leave the project's settings unchanged [#15396](https://github.com/pnpm/pnpm/issues/15396).
