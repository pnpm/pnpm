---
"@pnpm/deps.compliance.commands": minor
"pnpm": minor
---

`pnpm audit --fix` now respects the `auditLevel` setting and supports a new interactive mode via `--interactive`/`-i`. Previously, `pnpm audit --fix` would fix all vulnerabilities regardless of the configured `auditLevel`, while `pnpm audit` (without `--fix`) correctly filtered by severity. Now both commands consistently filter advisories by the `auditLevel` setting, and you can use `pnpm audit --fix -i` to review and select which vulnerabilities to fix interactively.

Overrides emitted by `pnpm audit --fix` now use a caret range (`^X.Y.Z`) instead of an open-ended `>=X.Y.Z`, so applying a security fix can no longer silently promote a dependency across a major version boundary.
