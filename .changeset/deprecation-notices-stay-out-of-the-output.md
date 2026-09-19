---
"@pnpm/core-loggers": major
"@pnpm/lockfile.types": minor
"@pnpm/cli.default-reporter": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/text.sanitize": patch
"pnpm": patch
"pacquet": patch
---

Install warnings no longer carry the text of a package's deprecation notice. The warning names the deprecated package and version, and the `pnpm:deprecation` event no longer carries the notice either. `pnpm view` still shows it on request.

`pnpm-lock.yaml` now records `deprecated: true` in place of the notice. A notice an older pnpm wrote there still reads as a deprecation.

pnpm strips control characters from the package name and version in a deprecation warning, and from the notice `pnpm outdated --long` prints.
