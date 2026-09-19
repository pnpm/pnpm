---
"@pnpm/core-loggers": minor
"@pnpm/lockfile.types": minor
"@pnpm/cli.default-reporter": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

pnpm no longer prints the text of a package's deprecation notice, which a publisher can rewrite on an already-published version. The warning still names the deprecated package and version, and `pnpm view <name>@<version>` shows the notice on request. The `pnpm:deprecation` event no longer carries it either.

`pnpm-lock.yaml` now records `deprecated: true` in place of the notice. A notice written there by an older pnpm still reads as a deprecation and is replaced by `true` the next time the entry changes.

`pnpm outdated --long` strips control characters from the notice it prints.
