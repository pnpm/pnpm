---
"@pnpm/core-loggers": major
"@pnpm/cli.default-reporter": patch
"@pnpm/deps.inspection.commands": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/text.sanitize": patch
"pnpm": patch
"pacquet": patch
---

Install warnings no longer carry the text of a package's deprecation notice. The warning names the deprecated package and version, and the `pnpm:deprecation` event no longer carries the notice either. `pnpm view` still shows it on request.

pnpm strips control characters from the package name and version in a deprecation warning, and from the notice `pnpm outdated --long` prints.

The text sanitizer now also strips the Unicode line and paragraph separators U+2028 and U+2029.
