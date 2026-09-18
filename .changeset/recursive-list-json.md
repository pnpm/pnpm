---
"@pnpm/deps.inspection.commands": patch
"@pnpm/deps.inspection.list": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -r list --json` now prints one JSON array. It printed a separate array for each project when `sharedWorkspaceLockfile` was `false`, so the output could not be parsed.

`pnpm -r list` now reads each project's own modules directory when the projects keep their own lockfiles, so `--long` and `--parseable` report the packages that project installed [#15011](https://github.com/pnpm/pnpm/issues/15011).
